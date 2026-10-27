//! O que o painel e o computador dizem um ao outro, depois do aperto de mão.
//!
//! O formato é `postcard`, posicional como o do protocolo: a ordem das variantes e dos campos
//! **é o formato de fio**. Acrescentar no fim é seguro; o resto exige subir [`VERSAO`]. Os vetores
//! gravados em `tests/vetores.rs` pegam a mudança esquecida.

use crate::amostra::UsageSample;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// A versão deste vocabulário. Independe da versão do protocolo entre os computadores: o painel e
/// o par não conversam, e um não precisa esperar o outro para mudar.
///
/// Versão 2: [`DoPainel::Interesse`] e [`ParaPainel::Detalhes`] — os processos e a cópia em curso
/// ([log 70](../../../docs/logs/70-processos-e-copia-no-painel.md)). Acrescentadas no fim: um
/// aplicativo da versão 1 nunca as recebe, porque não diz interesse.
pub const VERSAO: u16 = 2;

/// O maior nome que uma apresentação carrega, em bytes — o mesmo teto do nome de máquina.
pub const NOME_MAXIMO: usize = ir_proto::limits::MAX_MACHINE_NAME;

/// O que o painel diz ao computador. É **todo** o vocabulário dele: não há como pedir nada além
/// de ver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoPainel {
    /// A primeira mensagem, logo depois do aperto de mão.
    Ola(Apresentacao),
    /// Comece (`true`) ou pare de mandar o quadro a cada segundo. O painel pede ao aparecer na tela
    /// e desfaz ao sair dela.
    Assinar(bool),
    /// O que o painel quer além do quadro: os processos vigiados e a lista aberta. Desde a versão 2.
    Interesse(crate::detalhes::Interesse),
}

/// O que o computador diz ao painel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParaPainel {
    /// A resposta ao [`DoPainel::Ola`].
    Ola(Apresentacao),
    /// O quadro deste segundo.
    Quadro(Quadro),
    /// A cópia em curso e os processos, ao lado do quadro. Desde a versão 2, e só a quem disse
    /// [`DoPainel::Interesse`].
    Detalhes(crate::detalhes::Detalhes),
}

/// Quem fala, e em que versão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Apresentacao {
    /// A [`VERSAO`] de quem envia.
    pub versao: u16,
    /// O nome do aparelho ou do computador, para a outra tela.
    pub nome: String,
}

impl Apresentacao {
    /// A apresentação desta ponta, com o nome cortado no [`NOME_MAXIMO`].
    #[must_use]
    pub fn desta_ponta(nome: &str) -> Self {
        Self {
            versao: VERSAO,
            nome: crate::processos::cortar(nome, NOME_MAXIMO).to_owned(),
        }
    }
}

/// Tudo o que o painel mostra num segundo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quadro {
    /// O computador que agrega primeiro; o par, quando houver, depois.
    pub maquinas: Vec<Maquina>,
    /// O enlace entre os dois computadores, quando há sessão.
    pub enlace: Option<Enlace>,
}

/// Um computador no painel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Maquina {
    /// O nome dele.
    pub nome: String,
    /// O que se sabe dele agora.
    pub situacao: Situacao,
}

/// O que o painel sabe de um computador.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Situacao {
    /// A amostra deste segundo.
    Amostra(UsageSample),
    /// Conectado, e a primeira amostra ainda não chegou.
    Aguardando,
    /// O par é de uma versão que não manda amostra: precisa ser atualizado.
    VersaoAntiga,
    /// Sem sessão com o par agora.
    Desconectado,
}

/// O enlace entre os dois computadores: a volta por cada rota, e qual está ganhando.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enlace {
    /// A mediana das voltas pelo Bluetooth, em microssegundos; `None` sem Bluetooth na rota.
    pub bluetooth_us: Option<u32>,
    /// A mediana das voltas pela rede, em microssegundos; `None` sem rede na rota.
    pub rede_us: Option<u32>,
    /// Por onde chegou primeiro a maior parte do que chegou, na rota dupla.
    pub vencendo: Option<Rota>,
}

/// As duas rotas de entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rota {
    /// RFCOMM.
    Bluetooth,
    /// UDP na rede local.
    Rede,
}

/// Uma mensagem em bytes.
///
/// # Errors
///
/// Os do `postcard` — na prática, nenhum para os tipos deste módulo.
pub fn codificar<T: Serialize>(mensagem: &T) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(mensagem)
}

/// Bytes numa mensagem, sem tolerar sobra: o formato não é autodescritivo, e byte sobrando é
/// sinal de versões que não combinam.
///
/// # Errors
///
/// [`postcard::Error`] para bytes fora do formato, truncados ou com sobra.
pub fn decodificar<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, postcard::Error> {
    let (mensagem, resto) = postcard::take_from_bytes(bytes)?;
    if resto.is_empty() {
        Ok(mensagem)
    } else {
        Err(postcard::Error::DeserializeBadEncoding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ida_e_volta_de_um_quadro() {
        let quadro = ParaPainel::Quadro(Quadro {
            maquinas: vec![Maquina {
                nome: "notebook".to_owned(),
                situacao: Situacao::Aguardando,
            }],
            enlace: Some(Enlace {
                bluetooth_us: Some(7_000),
                rede_us: None,
                vencendo: Some(Rota::Bluetooth),
            }),
        });
        let bytes = codificar(&quadro).unwrap();
        assert_eq!(decodificar::<ParaPainel>(&bytes).unwrap(), quadro);
    }

    #[test]
    fn byte_sobrando_e_recusado() {
        let mut bytes = codificar(&DoPainel::Assinar(true)).unwrap();
        bytes.push(0);
        assert!(decodificar::<DoPainel>(&bytes).is_err());
    }

    #[test]
    fn nome_longo_e_cortado_sem_partir_caractere() {
        let nome = "é".repeat(NOME_MAXIMO);
        let apresentacao = Apresentacao::desta_ponta(&nome);
        assert!(apresentacao.nome.len() <= NOME_MAXIMO);
        assert!(apresentacao.nome.chars().all(|c| c == 'é'));
    }
}
