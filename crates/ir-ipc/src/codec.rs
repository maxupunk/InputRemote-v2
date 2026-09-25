//! Enquadramento das mensagens de IPC.
//!
//! Prefixo de tamanho de 4 bytes mais corpo em `postcard`. Simples de propósito: os dois lados
//! são nossos, rodam na mesma máquina, e não há negociação a fazer.
//!
//! O limite de tamanho existe pelo mesmo motivo que no protocolo de rede: o serviço roda
//! privilegiado, e um cliente local que anuncie uma mensagem de 4 GB não pode fazê-lo alocar.

use std::io::{Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Máximo de bytes numa mensagem de IPC.
///
/// Generoso para caber um relatório de diagnóstico e um arranjo de telas grande, e apertado o
/// bastante para que um pedido absurdo seja recusado antes de alocar.
pub const MAX_MENSAGEM: usize = 1 << 20;

/// Tamanho do prefixo, em bytes.
pub const PREFIXO: usize = 4;

/// O que pode dar errado ao enquadrar ou desenquadrar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ErroDeCodec {
    /// A mensagem passa do limite.
    #[error("mensagem de IPC com {tamanho} B, máximo {MAX_MENSAGEM} B")]
    GrandeDemais {
        /// O tamanho anunciado ou produzido.
        tamanho: usize,
    },
    /// Os bytes não formam a mensagem esperada.
    #[error("mensagem de IPC malformada")]
    Malformada,
    /// Faltam bytes para completar a mensagem.
    #[error("mensagem de IPC incompleta")]
    Incompleta,
}

/// Codifica uma mensagem com o prefixo de tamanho.
///
/// # Errors
///
/// [`ErroDeCodec::GrandeDemais`] se o corpo passar de [`MAX_MENSAGEM`];
/// [`ErroDeCodec::Malformada`] se a serialização falhar.
pub fn codificar<T: Serialize>(mensagem: &T) -> Result<Vec<u8>, ErroDeCodec> {
    let corpo = postcard::to_allocvec(mensagem).map_err(|_| ErroDeCodec::Malformada)?;
    if corpo.len() > MAX_MENSAGEM {
        return Err(ErroDeCodec::GrandeDemais {
            tamanho: corpo.len(),
        });
    }
    let tamanho = u32::try_from(corpo.len()).map_err(|_| ErroDeCodec::GrandeDemais {
        tamanho: corpo.len(),
    })?;

    let mut quadro = Vec::with_capacity(PREFIXO + corpo.len());
    quadro.extend_from_slice(&tamanho.to_le_bytes());
    quadro.extend_from_slice(&corpo);
    Ok(quadro)
}

/// Lê o tamanho anunciado por um prefixo, recusando o que passa do limite.
///
/// Separado da decodificação de propósito: quem lê de um socket precisa saber **quanto** ler
/// antes de ter os bytes, e precisa poder recusar um anúncio absurdo sem alocar nada.
///
/// # Errors
///
/// [`ErroDeCodec::Incompleta`] se o prefixo não estiver completo;
/// [`ErroDeCodec::GrandeDemais`] se o tamanho anunciado passar de [`MAX_MENSAGEM`].
pub fn tamanho_anunciado(prefixo: &[u8]) -> Result<usize, ErroDeCodec> {
    let bytes: [u8; PREFIXO] = prefixo
        .get(..PREFIXO)
        .and_then(|fatia| fatia.try_into().ok())
        .ok_or(ErroDeCodec::Incompleta)?;
    let tamanho = u32::from_le_bytes(bytes) as usize;
    if tamanho > MAX_MENSAGEM {
        return Err(ErroDeCodec::GrandeDemais { tamanho });
    }
    Ok(tamanho)
}

/// Decodifica o corpo de uma mensagem, sem o prefixo.
///
/// # Errors
///
/// [`ErroDeCodec::Malformada`] se os bytes não formarem a mensagem, ou se sobrar byte no fim.
pub fn decodificar<T: DeserializeOwned>(corpo: &[u8]) -> Result<T, ErroDeCodec> {
    let (valor, resto) =
        postcard::take_from_bytes::<T>(corpo).map_err(|_| ErroDeCodec::Malformada)?;
    if resto.is_empty() {
        Ok(valor)
    } else {
        Err(ErroDeCodec::Malformada)
    }
}

/// Codifica, escreve e esvazia uma mensagem num canal bloqueante.
///
/// É o que a janela, o agente, o ajudante de clipboard e a ferramenta de bancada fazem a cada
/// mensagem; o esvaziamento vai junto porque um quadro parado no *buffer* é um pedido que o serviço
/// nunca vê.
///
/// # Errors
///
/// O erro de E/S da escrita; um erro de [`codificar`] volta como [`std::io::ErrorKind::InvalidData`].
pub fn escrever_em<T: Serialize, W: Write + ?Sized>(
    escrita: &mut W,
    mensagem: &T,
) -> std::io::Result<()> {
    let quadro = codificar(mensagem).map_err(invalido)?;
    escrita.write_all(&quadro)?;
    escrita.flush()
}

/// Lê uma mensagem inteira de um canal bloqueante. `Ok(None)` no fim limpo do fluxo.
///
/// O tamanho anunciado é conferido contra o limite **antes** de alocar o corpo.
///
/// # Errors
///
/// O erro de E/S da leitura — um fluxo que acaba no meio do corpo inclusive; prefixo ou corpo
/// inválidos voltam como [`std::io::ErrorKind::InvalidData`].
pub fn ler_de<T: DeserializeOwned, R: Read + ?Sized>(
    leitura: &mut R,
) -> std::io::Result<Option<T>> {
    let mut prefixo = [0u8; PREFIXO];
    if let Err(erro) = leitura.read_exact(&mut prefixo) {
        return if erro.kind() == std::io::ErrorKind::UnexpectedEof {
            Ok(None)
        } else {
            Err(erro)
        };
    }
    let tamanho = tamanho_anunciado(&prefixo).map_err(invalido)?;
    let mut corpo = vec![0u8; tamanho];
    leitura.read_exact(&mut corpo)?;
    decodificar(&corpo).map(Some).map_err(invalido)
}

/// Um erro de enquadramento, no vocabulário de quem lê e escreve em canal.
fn invalido(erro: ErroDeCodec) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, erro)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Pedido;

    #[test]
    fn ida_e_volta_preserva_o_pedido() {
        let pedido = Pedido::FixarPortador(Some(crate::vocabulario::Portador::Bluetooth));
        let quadro = codificar(&pedido).expect("codifica");
        let tamanho = tamanho_anunciado(&quadro).expect("prefixo");
        let corpo = quadro.get(PREFIXO..PREFIXO + tamanho).expect("corpo");
        assert_eq!(decodificar::<Pedido>(corpo).expect("decodifica"), pedido);
    }

    #[test]
    fn o_prefixo_declara_o_tamanho_do_corpo() {
        let quadro = codificar(&Pedido::Estado).expect("codifica");
        let tamanho = tamanho_anunciado(&quadro).expect("prefixo");
        assert_eq!(quadro.len(), PREFIXO + tamanho);
    }

    #[test]
    fn um_prefixo_incompleto_e_recusado() {
        for parcial in 0..PREFIXO {
            let bytes = vec![0xFF; parcial];
            assert_eq!(tamanho_anunciado(&bytes), Err(ErroDeCodec::Incompleta));
        }
    }

    #[test]
    fn um_anuncio_absurdo_e_recusado_antes_de_alocar() {
        // O cenário que importa: um cliente local anuncia 4 GB. O serviço roda privilegiado e
        // não pode ser levado a alocar por um número que veio de fora.
        let prefixo = u32::MAX.to_le_bytes();
        assert!(matches!(
            tamanho_anunciado(&prefixo),
            Err(ErroDeCodec::GrandeDemais { .. })
        ));
    }

    #[test]
    fn byte_sobrando_no_corpo_e_erro() {
        let quadro = codificar(&Pedido::Estado).expect("codifica");
        let mut corpo = quadro.get(PREFIXO..).expect("corpo").to_vec();
        corpo.push(0);
        assert_eq!(decodificar::<Pedido>(&corpo), Err(ErroDeCodec::Malformada));
    }

    #[test]
    fn escrever_e_ler_num_canal_devolvem_a_mesma_mensagem() {
        let mut canal = Vec::new();
        escrever_em(&mut canal, &Pedido::Estado).expect("escreve");
        escrever_em(&mut canal, &Pedido::Acompanhar).expect("escreve");
        let mut leitura = canal.as_slice();
        assert_eq!(
            ler_de::<Pedido, _>(&mut leitura).expect("lê"),
            Some(Pedido::Estado)
        );
        assert_eq!(
            ler_de::<Pedido, _>(&mut leitura).expect("lê"),
            Some(Pedido::Acompanhar)
        );
        assert_eq!(
            ler_de::<Pedido, _>(&mut leitura).expect("fim"),
            None,
            "fim limpo"
        );
    }

    #[test]
    fn um_canal_que_acaba_no_meio_do_corpo_e_erro_e_nao_fim() {
        let mut canal = Vec::new();
        escrever_em(&mut canal, &Pedido::Estado).expect("escreve");
        // Um segundo quadro que promete nove bytes e entrega dois.
        canal.extend_from_slice(&[9, 0, 0, 0, 1, 2]);
        let mut leitura = canal.as_slice();
        assert!(
            ler_de::<Pedido, _>(&mut leitura)
                .expect("primeiro")
                .is_some()
        );
        assert!(ler_de::<Pedido, _>(&mut leitura).is_err());
    }

    #[test]
    fn um_anuncio_absurdo_no_canal_e_dado_invalido() {
        let prefixo = u32::MAX.to_le_bytes();
        let mut leitura = prefixo.as_slice();
        let erro = ler_de::<Pedido, _>(&mut leitura).expect_err("recusa");
        assert_eq!(erro.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn lixo_nao_gera_panico() {
        for semente in 0u16..=1500 {
            let bytes = semente.to_le_bytes();
            let _ = decodificar::<Pedido>(&bytes);
            let _ = tamanho_anunciado(&bytes);
        }
    }
}
