//! O enquadramento do RFCOMM. Parte pura, sem socket nem rádio.
//!
//! O RFCOMM entrega um *stream*: os bytes chegam em ordem e sem perda, mas picados e colados
//! como o rádio quiser. Não existe "um pacote, uma mensagem". Por isso
//! [03, §2](../../../docs/03-protocolo.md) manda um **prefixo de tamanho `u16`** antes de cada
//! corpo — é ele que devolve a fronteira que o meio não dá.
//!
//! # O contador não viaja
//!
//! É a diferença que mais importa em relação ao UDP. Lá o contador do Noise vai em claro no
//! quadro, *"porque a ordem não é garantida"* ([03, §3.1](../../../docs/03-protocolo.md)). Aqui
//! ela é garantida pelo meio, então quem recebe **conta** os quadros: o primeiro é 1, o
//! seguinte é 2, e assim por diante — exatamente a sequência que o
//! [`Transport`](ir_crypto::Transport) emite do outro lado.
//!
//! São 8 bytes a menos em cada quadro. Num portador cujo teto de texto claro é 512 B e cujos
//! quadros de entrada têm algumas dezenas de bytes, isso é perto de um quinto do canal.
//!
//! O preço é que as duas pontas precisam concordar na contagem. Como o meio não perde nem
//! reordena, elas só divergem se alguém adulterar os bytes — e aí a tag do Noise não confere, o
//! quadro não abre e o enlace cai. Divergir em silêncio não é um resultado possível.
//!
//! # Por que `Mode` e `Kind` estão escritos de novo
//!
//! São os mesmos do `ir-net`, e a duplicação é deliberada: o `xtask` só deixa o `ir-bt` depender
//! de `ir-proto` e `ir-crypto` ([02, §2](../../../docs/02-arquitetura.md)). Dois portadores não
//! se enxergam — é a seta que impede um transporte de virar dependência do outro. O que **não**
//! está duplicado é o que importa: a criptografia é o mesmo [`ir_crypto`], sem uma linha própria.

use ir_proto::limits;

use crate::error::{BtError, Result};

/// Bytes do prefixo de tamanho.
pub const PREFIXO: usize = 2;

/// O que o Noise acrescenta a cada quadro cifrado: a tag de 16 bytes.
const TAG: usize = 16;

/// O byte de espécie, no começo do texto claro.
const ESPECIE: usize = 1;

/// O maior corpo que pode vir depois do prefixo.
///
/// O teto do portador, mais o byte de espécie, mais a tag. Um `u16` chega a 65 535, então o
/// limite verdadeiro é este — e é contra ele que se confere **antes** de alocar qualquer coisa
/// ([04, §1](../../../docs/04-seguranca.md)).
pub const MAX_CORPO: usize = limits::MAX_RFCOMM_PLAINTEXT + ESPECIE + TAG;

/// O modo de um handshake, no primeiro byte do corpo.
///
/// Vai em claro porque quem recebe o primeiro corpo precisa escolher o padrão Noise antes de
/// decifrar coisa alguma.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    /// Primeiro pareamento — `Noise_XX`, seguido do código de 6 dígitos.
    Pair = 0,
    /// Reconexão — `Noise_IK`, com a chave do par fixada.
    Reconnect = 1,
}

impl Mode {
    /// Lê o modo do primeiro byte de um corpo de handshake.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Pair),
            1 => Some(Self::Reconnect),
            _ => None,
        }
    }

    /// O byte que representa este modo.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }
}

/// A espécie do texto claro cifrado, no primeiro byte de dentro do envelope.
///
/// Autenticada junto com o resto: um marcador de pareamento não pode ser forjado nem confundido
/// com um quadro de sessão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// Um quadro do protocolo (`ir_proto::Frame` codificado).
    SessionFrame = 0,
    /// O usuário confirmou que os códigos batem.
    PairConfirm = 1,
    /// O usuário disse que os códigos não batem, ou recusou.
    PairReject = 2,
}

impl Kind {
    /// Lê a espécie do primeiro byte do texto claro.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::SessionFrame),
            1 => Some(Self::PairConfirm),
            2 => Some(Self::PairReject),
            _ => None,
        }
    }

    /// O byte que representa esta espécie.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }
}

/// Põe o prefixo de tamanho num corpo, deixando-o pronto para o socket.
///
/// # Errors
///
/// [`BtError::GrandeDemais`] se o corpo passa de [`MAX_CORPO`]. Falhar aqui é falha nossa, e
/// custa menos que descobrir do outro lado.
pub fn enquadrar(corpo: &[u8]) -> Result<Vec<u8>> {
    let tamanho = u16::try_from(corpo.len())
        .ok()
        .filter(|_| corpo.len() <= MAX_CORPO);
    let Some(tamanho) = tamanho else {
        return Err(BtError::GrandeDemais {
            tamanho: corpo.len(),
            limite: MAX_CORPO,
        });
    };
    let mut saida = Vec::with_capacity(PREFIXO + corpo.len());
    saida.extend_from_slice(&tamanho.to_le_bytes());
    saida.extend_from_slice(corpo);
    Ok(saida)
}

/// Monta o corpo de um handshake: `[modo][mensagem Noise]`.
#[must_use]
pub fn corpo_de_handshake(modo: Mode, mensagem: &[u8]) -> Vec<u8> {
    let mut saida = Vec::with_capacity(1 + mensagem.len());
    saida.push(modo.to_byte());
    saida.extend_from_slice(mensagem);
    saida
}

/// Separa um corpo de handshake em modo e mensagem.
#[must_use]
pub fn ler_handshake(corpo: &[u8]) -> Option<(Mode, &[u8])> {
    let (primeiro, resto) = corpo.split_first()?;
    Some((Mode::from_byte(*primeiro)?, resto))
}

/// Embrulha um texto claro com o byte de espécie, para cifrar.
#[must_use]
pub fn embrulhar(especie: Kind, conteudo: &[u8]) -> Vec<u8> {
    let mut saida = Vec::with_capacity(1 + conteudo.len());
    saida.push(especie.to_byte());
    saida.extend_from_slice(conteudo);
    saida
}

/// Separa um texto claro decifrado em espécie e conteúdo.
#[must_use]
pub fn desembrulhar(texto_claro: &[u8]) -> Option<(Kind, &[u8])> {
    let (primeiro, resto) = texto_claro.split_first()?;
    Some((Kind::from_byte(*primeiro)?, resto))
}

/// Junta os pedaços que chegam do *stream* e devolve um corpo completo de cada vez.
///
/// É o tipo que sabe que o RFCOMM não respeita fronteira de mensagem. Alimente com o que o
/// socket entregou, do tamanho que vier, e peça corpos até não haver mais nenhum inteiro.
#[derive(Debug, Default)]
pub struct Desenquadrador {
    pendente: Vec<u8>,
}

impl Desenquadrador {
    /// Um desenquadrador vazio.
    #[must_use]
    pub const fn novo() -> Self {
        Self {
            pendente: Vec::new(),
        }
    }

    /// Guarda os bytes que acabaram de chegar do socket.
    pub fn alimentar(&mut self, bytes: &[u8]) {
        self.pendente.extend_from_slice(bytes);
    }

    /// Quantos bytes ainda não formaram um corpo inteiro.
    #[must_use]
    pub fn pendentes(&self) -> usize {
        self.pendente.len()
    }

    /// O próximo corpo completo, se já chegou inteiro.
    ///
    /// # Errors
    ///
    /// [`BtError::GrandeDemais`] se o tamanho anunciado passa de [`MAX_CORPO`]. Conferido
    /// **antes** de reservar memória: o serviço é privilegiado e estes bytes vêm de um rádio que
    /// qualquer um alcança ([04, §1](../../../docs/04-seguranca.md)). Um anúncio absurdo derruba
    /// o enlace, e não a máquina.
    pub fn proximo(&mut self) -> Result<Option<Vec<u8>>> {
        let Some(prefixo) = self.pendente.get(..PREFIXO) else {
            return Ok(None); // nem o tamanho chegou ainda
        };
        let Ok(bytes) = <[u8; PREFIXO]>::try_from(prefixo) else {
            return Ok(None);
        };
        let tamanho = usize::from(u16::from_le_bytes(bytes));
        if tamanho > MAX_CORPO {
            return Err(BtError::GrandeDemais {
                tamanho,
                limite: MAX_CORPO,
            });
        }
        let fim = PREFIXO + tamanho;
        if self.pendente.len() < fim {
            return Ok(None); // o corpo ainda está chegando
        }
        let corpo: Vec<u8> = self.pendente.drain(..fim).skip(PREFIXO).collect();
        Ok(Some(corpo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn um_corpo_enquadrado_volta_igual() {
        let quadro = enquadrar(b"corpo").expect("cabe");
        let mut des = Desenquadrador::novo();
        des.alimentar(&quadro);
        assert_eq!(des.proximo().expect("válido"), Some(b"corpo".to_vec()));
        assert_eq!(des.proximo().expect("válido"), None);
    }

    #[test]
    fn um_corpo_partido_byte_a_byte_ainda_se_monta() {
        // O caso que só existe em stream: o rádio entrega os bytes como quiser, e a mensagem
        // não tem fronteira própria.
        let quadro = enquadrar(b"tecla pressionada").expect("cabe");
        let mut des = Desenquadrador::novo();
        for byte in &quadro {
            assert_eq!(des.proximo().expect("válido"), None, "ainda incompleto");
            des.alimentar(&[*byte]);
        }
        assert_eq!(
            des.proximo().expect("válido"),
            Some(b"tecla pressionada".to_vec())
        );
    }

    #[test]
    fn varios_corpos_colados_numa_leitura_so_saem_um_a_um() {
        // O outro lado do mesmo problema: o rádio cola mensagens numa entrega só.
        let mut fluxo = Vec::new();
        for corpo in [&b"um"[..], b"dois", b"tres"] {
            fluxo.extend_from_slice(&enquadrar(corpo).expect("cabe"));
        }
        let mut des = Desenquadrador::novo();
        des.alimentar(&fluxo);
        assert_eq!(des.proximo().expect("válido"), Some(b"um".to_vec()));
        assert_eq!(des.proximo().expect("válido"), Some(b"dois".to_vec()));
        assert_eq!(des.proximo().expect("válido"), Some(b"tres".to_vec()));
        assert_eq!(des.proximo().expect("válido"), None);
        assert_eq!(des.pendentes(), 0);
    }

    #[test]
    fn um_corpo_vazio_e_um_corpo_valido() {
        // `PairConfirm` viaja sem conteúdo: só a espécie, e ela vai cifrada.
        let quadro = enquadrar(b"").expect("cabe");
        assert_eq!(quadro, vec![0, 0]);
        let mut des = Desenquadrador::novo();
        des.alimentar(&quadro);
        assert_eq!(des.proximo().expect("válido"), Some(Vec::new()));
    }

    #[test]
    fn um_tamanho_absurdo_e_recusado_antes_de_alocar() {
        // A regra de docs/04 §1. Estes dois bytes vêm de um rádio que qualquer um alcança, e o
        // processo que os lê é privilegiado.
        let mut des = Desenquadrador::novo();
        des.alimentar(&u16::MAX.to_le_bytes());
        let erro = des.proximo().expect_err("precisa recusar");
        assert!(matches!(
            erro,
            BtError::GrandeDemais {
                tamanho: 65_535,
                limite: MAX_CORPO
            }
        ));
    }

    #[test]
    fn o_maior_corpo_legitimo_passa_e_o_seguinte_nao() {
        // A fronteira exata, dos dois lados: um quadro cheio de verdade precisa caber.
        let cheio = vec![0xAB; MAX_CORPO];
        let quadro = enquadrar(&cheio).expect("o maior corpo legítimo cabe");
        let mut des = Desenquadrador::novo();
        des.alimentar(&quadro);
        assert_eq!(des.proximo().expect("válido"), Some(cheio));

        let grande = vec![0u8; MAX_CORPO + 1];
        assert!(matches!(
            enquadrar(&grande),
            Err(BtError::GrandeDemais { .. })
        ));
    }

    #[test]
    fn o_teto_do_corpo_cobre_um_quadro_de_sessao_cheio() {
        // O teto não é um número solto: é o teto do portador, mais a espécie, mais a tag. Se o
        // `ir-proto` mudar o limite do RFCOMM, este teste é quem avisa.
        assert_eq!(MAX_CORPO, limits::MAX_RFCOMM_PLAINTEXT + 1 + 16);
        let texto_claro = embrulhar(Kind::SessionFrame, &vec![0u8; limits::MAX_RFCOMM_PLAINTEXT]);
        assert_eq!(texto_claro.len() + 16, MAX_CORPO);
    }

    #[test]
    fn o_handshake_sobrevive_a_ida_e_volta() {
        let corpo = corpo_de_handshake(Mode::Pair, b"mensagem-noise");
        assert_eq!(
            ler_handshake(&corpo),
            Some((Mode::Pair, &b"mensagem-noise"[..]))
        );
    }

    #[test]
    fn o_embrulho_sobrevive_a_ida_e_volta() {
        for especie in [Kind::SessionFrame, Kind::PairConfirm, Kind::PairReject] {
            let texto = embrulhar(especie, b"conteudo");
            assert_eq!(desembrulhar(&texto), Some((especie, &b"conteudo"[..])));
        }
    }

    #[test]
    fn modo_e_especie_desconhecidos_sao_recusados() {
        assert_eq!(Mode::from_byte(9), None);
        assert_eq!(Kind::from_byte(9), None);
        assert_eq!(ler_handshake(&[9, 0]), None);
        assert_eq!(desembrulhar(&[9, 0]), None);
        assert_eq!(ler_handshake(&[]), None);
        assert_eq!(desembrulhar(&[]), None);
    }

    #[test]
    fn modo_e_especie_sobrevivem_a_ida_e_volta_pelos_bytes() {
        for modo in [Mode::Pair, Mode::Reconnect] {
            assert_eq!(Mode::from_byte(modo.to_byte()), Some(modo));
        }
        for especie in [Kind::SessionFrame, Kind::PairConfirm, Kind::PairReject] {
            assert_eq!(Kind::from_byte(especie.to_byte()), Some(especie));
        }
    }
}
