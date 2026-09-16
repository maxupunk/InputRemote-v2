//! O canal de bytes, e a leitura por quadros em cima dele.
//!
//! Este módulo é a fronteira entre o que precisa de rádio e o que não precisa. Acima dele —
//! handshake, enlace cifrado, máquina de estados — tudo trabalha contra [`Canal`], que é só
//! "um fluxo de bytes nos dois sentidos". Abaixo dele ficam os backends de cada sistema.
//!
//! O efeito prático é que o protocolo inteiro é testável com [`tokio::io::duplex`], sem dois
//! computadores e sem rádio nenhum. Foi a impossibilidade de fazer isso que deixou o
//! pareamento por Bluetooth do v1 sem um único teste
//! ([00, §6](../../../docs/00-licoes-do-v1.md)).
//!
//! # Por que `AsyncRead`/`AsyncWrite`, e não um *trait* próprio
//!
//! Porque já é o que os dois backends falam. No Linux, `bluer::rfcomm::Stream` implementa os
//! dois. No Windows, o Winsock `AF_BTH` é síncrono e ganha uma ponte que implementa os dois.
//! Um *trait* próprio obrigaria a escrever adaptadores para os dois lados e tiraria o
//! `tokio::io::duplex` dos testes, que é justamente o que dá o teste de graça.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{BtError, Result};
use crate::wire::{self, Desenquadrador, MAX_CORPO, PREFIXO};

/// Um fluxo de bytes bidirecional com o par.
///
/// Não há nada a implementar: quem já é `AsyncRead + AsyncWrite` serve. O *trait* existe para
/// dar um nome a essa exigência e para o resto do crate não repetir quatro limites em cada
/// assinatura.
pub trait Canal: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Canal for T {}

/// Quanto se lê do canal de uma vez.
///
/// Um quadro inteiro com folga. Ler de mais não custa — o que sobrar fica no desenquadrador e
/// vira o quadro seguinte.
const LEITURA: usize = MAX_CORPO + PREFIXO;

/// Um canal visto como uma sequência de quadros, e não de bytes.
///
/// Resolve, num lugar só, o que o RFCOMM não resolve: onde uma mensagem termina e a próxima
/// começa ([03, §2](../../../docs/03-protocolo.md)).
#[derive(Debug)]
pub struct Quadros<C> {
    canal: C,
    desenquadrador: Desenquadrador,
    buffer: Box<[u8]>,
}

impl<C: Canal> Quadros<C> {
    /// Envolve um canal.
    #[must_use]
    pub fn novo(canal: C) -> Self {
        Self {
            canal,
            desenquadrador: Desenquadrador::novo(),
            buffer: vec![0u8; LEITURA].into_boxed_slice(),
        }
    }

    /// Devolve o canal, para fechá-lo ou repassá-lo.
    #[must_use]
    pub fn em_bytes(self) -> C {
        self.canal
    }

    /// Manda um corpo, com o prefixo de tamanho na frente.
    ///
    /// # Errors
    ///
    /// [`BtError::GrandeDemais`] se o corpo passa do teto do portador; [`BtError::Io`] se o
    /// socket falhar.
    pub async fn enviar(&mut self, corpo: &[u8]) -> Result<()> {
        let quadro = wire::enquadrar(corpo)?;
        self.canal.write_all(&quadro).await?;
        // Sem `flush` o corpo pode ficar num buffer intermediário, e do outro lado o handshake
        // esperaria por uma mensagem que já foi escrita. Num protocolo de passo a passo, isso é
        // um impasse, não um atraso.
        self.canal.flush().await?;
        Ok(())
    }

    /// Espera o próximo corpo completo.
    ///
    /// # Errors
    ///
    /// [`BtError::SemResposta`] se o par fechou o canal; [`BtError::GrandeDemais`] se ele
    /// anunciou um tamanho absurdo; [`BtError::Io`] em falha de socket.
    pub async fn receber(&mut self) -> Result<Vec<u8>> {
        loop {
            if let Some(corpo) = self.desenquadrador.proximo()? {
                return Ok(corpo);
            }
            let lidos = self.canal.read(&mut self.buffer).await?;
            let Some(chegaram) = self.buffer.get(..lidos).filter(|_| lidos > 0) else {
                // Leitura de zero byte é fim de fluxo: o par fechou. Não é erro de socket, e
                // dizer "o par não atendeu" é o que o usuário precisa ouvir.
                return Err(BtError::SemResposta);
            };
            self.desenquadrador.alimentar(chegaram);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um par de canais ligados um no outro, como dois computadores.
    fn par() -> (
        Quadros<tokio::io::DuplexStream>,
        Quadros<tokio::io::DuplexStream>,
    ) {
        let (a, b) = tokio::io::duplex(4096);
        (Quadros::novo(a), Quadros::novo(b))
    }

    #[tokio::test]
    async fn um_corpo_enviado_chega_inteiro() {
        let (mut aqui, mut la) = par();
        aqui.enviar(b"ctrl pressionado").await.expect("envia");
        assert_eq!(la.receber().await.expect("recebe"), b"ctrl pressionado");
    }

    #[tokio::test]
    async fn varios_corpos_mantem_a_ordem_e_a_fronteira() {
        // O ponto do enquadramento: mesmo colados num fluxo só, saem separados e na ordem.
        let (mut aqui, mut la) = par();
        for corpo in [&b"um"[..], b"dois", b"", b"quatro"] {
            aqui.enviar(corpo).await.expect("envia");
        }
        for esperado in [&b"um"[..], b"dois", b"", b"quatro"] {
            assert_eq!(la.receber().await.expect("recebe"), esperado);
        }
    }

    #[tokio::test]
    async fn um_corpo_do_tamanho_maximo_atravessa() {
        let (mut aqui, mut la) = par();
        let cheio = vec![0x5A; MAX_CORPO];
        aqui.enviar(&cheio).await.expect("envia");
        assert_eq!(la.receber().await.expect("recebe"), cheio);
    }

    #[tokio::test]
    async fn enviar_alem_do_teto_falha_sem_escrever_nada() {
        // Falhar aqui é defeito nosso, e custa menos que o par receber um quadro que ele vai
        // recusar. O canal não pode ficar sujo com meia mensagem.
        let (mut aqui, mut la) = par();
        let erro = aqui
            .enviar(&vec![0u8; MAX_CORPO + 1])
            .await
            .expect_err("precisa recusar");
        assert!(matches!(erro, BtError::GrandeDemais { .. }));

        aqui.enviar(b"seguinte").await.expect("envia");
        assert_eq!(
            la.receber().await.expect("recebe"),
            b"seguinte",
            "o canal continuou limpo"
        );
    }

    #[tokio::test]
    async fn o_par_que_fecha_vira_sem_resposta_e_nao_erro_de_socket() {
        // É a diferença que o ADR-0005 exige: "pareado, mas o serviço não responde" precisa ser
        // dizível para o usuário.
        let (aqui, mut la) = par();
        drop(aqui);
        assert!(matches!(
            la.receber().await.expect_err("fim de fluxo"),
            BtError::SemResposta
        ));
    }

    #[tokio::test]
    async fn um_tamanho_absurdo_derruba_o_enlace_e_nao_a_maquina() {
        // Bytes hostis vindos do rádio, lidos por um processo privilegiado: o tamanho é
        // conferido antes de qualquer alocação (docs/04 §1).
        let (mut bruto, mut la) = tokio::io::duplex(64);
        tokio::io::AsyncWriteExt::write_all(&mut bruto, &u16::MAX.to_le_bytes())
            .await
            .expect("escreve");
        let mut quadros = Quadros::novo(&mut la);
        assert!(matches!(
            quadros.receber().await.expect_err("precisa recusar"),
            BtError::GrandeDemais { .. }
        ));
    }
}
