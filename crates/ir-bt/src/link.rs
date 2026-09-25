//! O enlace cifrado sobre RFCOMM: cifra ao enviar, decifra ao receber.
//!
//! Faz o que o `SecureLink` do `ir-net` faz, com **uma** diferença, e é toda a diferença entre
//! os dois portadores: aqui o contador do Noise não viaja.
//!
//! # O contador contado, e não recebido
//!
//! O contador é o nonce. No UDP ele vai em claro no quadro, porque a ordem não é garantida e
//! quem recebe não tem como saber qual quadro é este. No RFCOMM a ordem é garantida pelo meio,
//! então quem recebe **conta**: o primeiro quadro é 1, o seguinte é 2, e essa é exatamente a
//! sequência que o [`Transport`] do outro lado emite ([03, §3.1](../../../docs/03-protocolo.md)).
//!
//! Isso economiza 8 bytes por quadro num portador que tem 512 B para trabalhar, e não enfraquece
//! nada: um contador contado localmente é ainda mais difícil de manipular que um recebido do
//! outro lado. A [`ReplayWindow`](ir_crypto::ReplayWindow) do transporte continua no caminho.
//!
//! **A contagem só avança quando a tag confere.** Um quadro adulterado não queima o número do
//! legítimo — a mesma disciplina que o [`Transport::open`] já aplica à janela de repetição. A regra
//! mora no [`ContadorImplicito`], o mesmo do TCP de arquivos.
//!
//! # Sem troca de chaves
//!
//! O rádio ainda não troca chaves com o enlace de pé, como a rede faz depois de 2^20 quadros ou
//! dez minutos ([03, §3](../../../docs/03-protocolo.md)). Havia aqui uma consulta ao contador que
//! ninguém chamava; saiu, para não parecer que a troca existe. As chaves só se renovam quando o
//! enlace cai e é rediscado — é uma pendência, não uma decisão.

use ir_crypto::Transport;
use ir_crypto::enlace::ContadorImplicito;

use crate::canal::{Canal, Quadros};
use crate::error::Result;
use crate::wire::{self, Kind};

/// Um enlace cifrado com o par, sobre um canal RFCOMM.
#[derive(Debug)]
pub struct EnlaceSeguro<C> {
    quadros: Quadros<C>,
    transporte: Transport,
    /// Quantos quadros já foram abertos. O próximo contador é este mais um.
    contagem: ContadorImplicito,
}

impl<C: Canal> EnlaceSeguro<C> {
    /// Monta o enlace a partir do transporte que o handshake produziu.
    #[must_use]
    pub const fn novo(quadros: Quadros<C>, transporte: Transport) -> Self {
        Self {
            quadros,
            transporte,
            contagem: ContadorImplicito::novo(),
        }
    }

    /// Cifra e envia um texto claro da espécie dada.
    ///
    /// # Errors
    ///
    /// [`BtError::Crypto`](crate::BtError::Crypto) se a cifragem falhar;
    /// [`BtError::GrandeDemais`](crate::BtError::GrandeDemais) se o resultado passar do teto do
    /// portador; [`BtError::Io`](crate::BtError::Io) em falha de socket.
    pub async fn enviar(&mut self, especie: Kind, conteudo: &[u8]) -> Result<()> {
        let texto_claro = wire::embrulhar(especie, conteudo);
        // O contador devolvido é descartado de propósito: ele não vai para o fio. O outro lado
        // chega ao mesmo número contando, e é isso que torna o quadro 8 bytes menor.
        let (_contador, cifrado) = self.transporte.seal(&texto_claro)?;
        self.quadros.enviar(&cifrado).await
    }

    /// Espera o próximo quadro do par, já decifrado.
    ///
    /// # Errors
    ///
    /// [`BtError::SemResposta`](crate::BtError::SemResposta) se o par fechou;
    /// [`BtError::Crypto`](crate::BtError::Crypto) se a tag não conferir — o que sobre um meio
    /// confiável e ordenado significa adulteração, e não perda;
    /// [`BtError::Malformed`](crate::BtError::Malformed) se o texto claro não trouxer uma
    /// espécie conhecida.
    pub async fn receber(&mut self) -> Result<(Kind, Vec<u8>)> {
        let cifrado = self.quadros.receber().await?;
        self.abrir(&cifrado)
    }

    /// Decifra um corpo já lido do canal.
    fn abrir(&mut self, cifrado: &[u8]) -> Result<(Kind, Vec<u8>)> {
        // A contagem só avança se a tag conferir: um quadro que não abriu não consome o número
        // do próximo.
        let transporte = &mut self.transporte;
        let texto_claro = self
            .contagem
            .abrir(|contador| transporte.open(contador, cifrado))?;
        let (especie, conteudo) =
            wire::desembrulhar(&texto_claro).ok_or(crate::BtError::Malformed)?;
        Ok((especie, conteudo.to_vec()))
    }

    /// Quantos quadros já foram abertos neste enlace.
    #[must_use]
    pub const fn recebidos(&self) -> u64 {
        self.contagem.recebidos()
    }
}

#[cfg(test)]
mod tests {
    use ir_crypto::{Handshake, Identity};
    use tokio::io::DuplexStream;

    use super::*;
    use crate::error::BtError;

    /// Dois enlaces cifrados ligados um no outro, com o handshake já feito.
    fn par() -> (EnlaceSeguro<DuplexStream>, EnlaceSeguro<DuplexStream>) {
        let (ia, ib) = (Identity::generate(), Identity::generate());
        let mut ha = Handshake::pair_initiator(&ia).expect("inicia");
        let mut hb = Handshake::pair_responder(&ib).expect("responde");
        for _ in 0..6 {
            if ha.is_finished() && hb.is_finished() {
                break;
            }
            if ha.is_my_turn() {
                let m = ha.write_message().expect("escreve");
                hb.read_message(&m).expect("lê");
            } else if hb.is_my_turn() {
                let m = hb.write_message().expect("escreve");
                ha.read_message(&m).expect("lê");
            }
        }
        let (ca, cb) = tokio::io::duplex(8192);
        (
            EnlaceSeguro::novo(Quadros::novo(ca), ha.into_transport().expect("transporte")),
            EnlaceSeguro::novo(Quadros::novo(cb), hb.into_transport().expect("transporte")),
        )
    }

    #[tokio::test]
    async fn um_quadro_cifrado_chega_como_saiu() {
        let (mut aqui, mut la) = par();
        aqui.enviar(Kind::SessionFrame, b"tecla")
            .await
            .expect("envia");
        assert_eq!(
            la.receber().await.expect("recebe"),
            (Kind::SessionFrame, b"tecla".to_vec())
        );
    }

    #[tokio::test]
    async fn a_contagem_implicita_acompanha_muitos_quadros() {
        // O coração do módulo: sem contador no fio, as duas pontas precisam chegar ao mesmo
        // número sozinhas, quadro após quadro.
        let (mut aqui, mut la) = par();
        for n in 0..200u32 {
            aqui.enviar(Kind::SessionFrame, &n.to_le_bytes())
                .await
                .expect("envia");
        }
        for n in 0..200u32 {
            let (especie, conteudo) = la.receber().await.expect("recebe");
            assert_eq!(especie, Kind::SessionFrame);
            assert_eq!(conteudo, n.to_le_bytes(), "o quadro {n} saiu de ordem");
        }
        assert_eq!(la.recebidos(), 200);
    }

    #[tokio::test]
    async fn as_tres_especies_atravessam() {
        let (mut aqui, mut la) = par();
        for especie in [Kind::SessionFrame, Kind::PairConfirm, Kind::PairReject] {
            aqui.enviar(especie, b"").await.expect("envia");
            assert_eq!(la.receber().await.expect("recebe").0, especie);
        }
    }

    #[tokio::test]
    async fn um_quadro_adulterado_nao_abre_e_nao_queima_o_contador() {
        // Num meio confiável e ordenado, tag que não confere significa adulteração. O quadro
        // seguinte, legítimo, ainda precisa abrir — senão um byte trocado por um terceiro
        // derrubaria o enlace para sempre em vez de só descartar o quadro.
        let (mut aqui, mut la) = par();
        let texto = wire::embrulhar(Kind::SessionFrame, b"senha");
        let (_, cifrado) = aqui.transporte.seal(&texto).expect("cifra");
        let mut adulterado = cifrado.clone();
        if let Some(primeiro) = adulterado.first_mut() {
            *primeiro ^= 0xFF;
        }

        assert!(matches!(
            la.abrir(&adulterado).expect_err("a tag não confere"),
            BtError::Crypto(_)
        ));
        assert_eq!(la.recebidos(), 0, "o contador não pode ter avançado");
        assert_eq!(
            la.abrir(&cifrado).expect("o legítimo ainda abre"),
            (Kind::SessionFrame, b"senha".to_vec())
        );
        assert_eq!(la.recebidos(), 1);
    }

    #[tokio::test]
    async fn um_quadro_repetido_e_recusado() {
        // A janela de repetição continua no caminho: reenviar um `KeyDown` capturado do ar não
        // pode funcionar, num produto que digita senhas (docs/03 §3.1).
        let (mut aqui, mut la) = par();
        let texto = wire::embrulhar(Kind::SessionFrame, b"x");
        let (_, cifrado) = aqui.transporte.seal(&texto).expect("cifra");
        assert!(la.abrir(&cifrado).is_ok());
        assert!(
            la.abrir(&cifrado).is_err(),
            "o mesmo quadro não pode ser aceito de novo"
        );
    }

    #[tokio::test]
    async fn o_contador_nao_viaja_no_fio() {
        // A prova de que a economia é real, lida nos bytes crus: o quadro tem prefixo, espécie,
        // conteúdo e tag — e nada mais. Com o contador explícito do UDP seriam 8 bytes a mais.
        // prefixo(2) + espécie(1) + conteúdo(8) + tag(16). Com o contador seriam 8 bytes a mais.
        const ESPERADO: usize = 2 + 1 + 8 + 16;

        let (mut aqui, la) = par();
        let mut bruto = la.quadros.em_bytes();
        let conteudo = b"12345678";
        aqui.enviar(Kind::SessionFrame, conteudo)
            .await
            .expect("envia");

        let mut quadro = [0u8; ESPERADO];
        tokio::io::AsyncReadExt::read_exact(&mut bruto, &mut quadro)
            .await
            .expect("lê o quadro inteiro");

        let anunciado = u16::from_le_bytes([quadro[0], quadro[1]]);
        assert_eq!(
            usize::from(anunciado),
            1 + conteudo.len() + 16,
            "o corpo é espécie + conteúdo + tag, sem contador"
        );
    }
}
