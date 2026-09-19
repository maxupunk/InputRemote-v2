//! Quem estÃ¡ por perto para parear: a rede (a pergunta por broadcast do `ir-net`), o rÃ¡dio (pareados
//! no sistema) e o endereÃ§o que alguÃ©m escreveu na configuraÃ§Ã£o.
//!
//! Antes, "Procurar" devolvia sÃ³ o `peer_addr` do arquivo de configuraÃ§Ã£o. Numa instalaÃ§Ã£o limpa ele
//! nÃ£o existe, e a lista vinha vazia; com um que sobrou de outra rede, ela mostrava um computador que
//! nÃ£o estava lÃ¡. A lista de pareados do sistema jÃ¡ existia no `ir-bt`, e ninguÃ©m a chamava; a
//! pergunta na rede nasceu junto com este mÃ³dulo.
//!
//! Este mÃ³dulo mora aqui, e nÃ£o no serviÃ§o, pela mesma razÃ£o do resto do crate: Ã© o Ãºnico lugar que
//! conhece a rede e o rÃ¡dio ao mesmo tempo ([02, Â§2](../../../docs/02-arquitetura.md)).

use std::time::Duration;

use ir_bt::Dispositivo;
use ir_net::{Anuncio, Candidate};
use ir_proto::ids::MachineId;

use crate::{Endereco, Pareados};

/// Um computador que a busca encontrou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encontrado {
    /// Como mostrar na tela.
    pub rotulo: String,
    /// Onde discar para parear.
    pub endereco: Endereco,
}

/// Quanto tempo uma busca escuta a rede. Na mesma rede a resposta chega em milissegundos; a folga
/// Ã© para o Wi-Fi em economia de energia, que atrasa broadcast.
const DURACAO_DA_BUSCA: Duration = Duration::from_secs(2);

/// A descoberta desta mÃ¡quina: responde a quem pergunta, e pergunta pelos outros.
#[derive(Debug, Clone)]
pub struct Descoberta {
    propria: String,
    pareados: Option<Pareados>,
}

impl Descoberta {
    /// Uma descoberta desta mÃ¡quina, ainda sem anÃºncio. `pareados` Ã© a alÃ§a do rÃ¡dio, quando hÃ¡.
    #[must_use]
    pub fn nova(maquina: MachineId, pareados: Option<Pareados>) -> Self {
        Self {
            propria: id_de(maquina),
            pareados,
        }
    }

    /// Uma descoberta que nÃ£o acha nada: para a bancada de testes do serviÃ§o.
    #[must_use]
    pub fn desligada() -> Self {
        Self {
            propria: String::new(),
            pareados: None,
        }
    }

    /// Passa a responder a quem perguntar na rede, com o nome desta mÃ¡quina e a porta do serviÃ§o.
    ///
    /// # Errors
    ///
    /// Se a porta da descoberta nÃ£o abrir. Os outros nÃ£o acham esta mÃ¡quina sozinhos, mas o rÃ¡dio e
    /// o endereÃ§o digitado continuam funcionando, e esta ainda acha as outras.
    pub async fn anunciar(&self, nome: &str, porta: u16) -> ir_net::Result<()> {
        let socket = ir_net::descoberta::abrir_resposta().await?;
        let anuncio = Anuncio {
            maquina: self.propria.clone(),
            nome: nome.to_owned(),
            porta,
        };
        tokio::spawn(ir_net::descoberta::responder(socket, anuncio));
        Ok(())
    }

    /// Procura na rede e no rÃ¡dio, e junta com o endereÃ§o configurado.
    ///
    /// Devolve um futuro que nÃ£o depende de `self`, para rodar numa tarefa prÃ³pria: a busca leva
    /// segundos, e o ator do serviÃ§o nÃ£o pode esperar nada. Nunca falha: uma fonte que nÃ£o responde
    /// sÃ³ nÃ£o contribui. A lista vazia Ã© uma resposta â€” e a janela diz o que fazer com ela.
    pub fn procurar(
        &self,
        configurado: Option<Endereco>,
    ) -> impl Future<Output = Vec<Encontrado>> + Send + 'static {
        let (pareados, propria) = (self.pareados.clone(), self.propria.clone());
        async move {
            let rede = async {
                ir_net::descoberta::procurar(DURACAO_DA_BUSCA)
                    .await
                    .unwrap_or_default()
            };
            let radio = async {
                match pareados {
                    Some(pareados) => pareados.listar().await,
                    None => Vec::new(),
                }
            };
            let (rede, radio) = tokio::join!(rede, radio);
            juntar(&propria, rede, radio, configurado)
        }
    }
}

/// O identificador anunciado, em hexadecimal: o mesmo texto dos dois lados, para esta mÃ¡quina se
/// reconhecer na prÃ³pria busca.
fn id_de(maquina: MachineId) -> String {
    use std::fmt::Write as _;
    maquina.0.iter().fold(String::new(), |mut texto, byte| {
        let _ = write!(texto, "{byte:02x}");
        texto
    })
}

/// Junta as fontes numa lista sÃ³, na ordem em que vale a pena tentar.
///
/// Rede primeiro: quem responde Ã  pergunta Ã© o InputRemote, com certeza. Depois o rÃ¡dio: a lista de pareados do
/// sistema traz tambÃ©m fones e mouses, entÃ£o o nome do dispositivo vai junto para a pessoa
/// reconhecer o computador. Por Ãºltimo o endereÃ§o da configuraÃ§Ã£o, marcado como tal â€” pode ser de
/// outra rede. Esta prÃ³pria mÃ¡quina e endereÃ§os repetidos saem.
#[must_use]
pub fn juntar(
    propria: &str,
    rede: Vec<Candidate>,
    pareados: Vec<Dispositivo>,
    configurado: Option<Endereco>,
) -> Vec<Encontrado> {
    let mut lista: Vec<Encontrado> = Vec::new();
    let mut acrescentar = |encontrado: Encontrado| {
        if !lista.iter().any(|ja| ja.endereco == encontrado.endereco) {
            lista.push(encontrado);
        }
    };
    for candidato in rede.into_iter().filter(|c| c.machine != propria) {
        acrescentar(Encontrado {
            rotulo: candidato.label,
            endereco: Endereco::Rede(candidato.addr),
        });
    }
    for dispositivo in pareados {
        acrescentar(Encontrado {
            rotulo: dispositivo.nome,
            endereco: Endereco::Radio(dispositivo.endereco),
        });
    }
    if let Some(endereco) = configurado {
        acrescentar(Encontrado {
            rotulo: format!("EndereÃ§o configurado: {endereco}"),
            endereco,
        });
    }
    lista
}

#[cfg(test)]
mod tests {
    use ir_bt::BdAddr;

    use super::*;

    fn na_rede(nome: &str, maquina: &str, endereco: &str) -> Candidate {
        Candidate {
            label: nome.to_owned(),
            machine: maquina.to_owned(),
            addr: endereco.parse().unwrap(),
        }
    }

    fn pareado(nome: &str, byte: u8) -> Dispositivo {
        Dispositivo {
            endereco: BdAddr([byte; 6]),
            nome: nome.to_owned(),
            conectado: false,
        }
    }

    #[test]
    fn a_propria_maquina_nao_aparece_na_propria_busca() {
        let lista = juntar(
            "aaaa",
            vec![
                na_rede("este", "aaaa", "10.0.0.2:52525"),
                na_rede("outro", "bbbb", "10.0.0.3:52525"),
            ],
            Vec::new(),
            None,
        );
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].rotulo, "outro");
    }

    #[test]
    fn rede_antes_de_radio_antes_do_configurado() {
        let lista = juntar(
            "aaaa",
            vec![na_rede("fedora", "bbbb", "10.0.0.135:52525")],
            vec![pareado("SAMSUNG-MAXUEL", 7)],
            Endereco::ler("10.0.0.99:52525"),
        );
        let rotulos: Vec<&str> = lista.iter().map(|e| e.rotulo.as_str()).collect();
        assert_eq!(
            rotulos,
            [
                "fedora",
                "SAMSUNG-MAXUEL",
                "EndereÃ§o configurado: 10.0.0.99:52525"
            ]
        );
    }

    #[test]
    fn o_configurado_que_a_rede_ja_achou_nao_aparece_duas_vezes() {
        let lista = juntar(
            "aaaa",
            vec![na_rede("fedora", "bbbb", "10.0.0.135:52525")],
            Vec::new(),
            Endereco::ler("10.0.0.135:52525"),
        );
        assert_eq!(lista.len(), 1);
        assert_eq!(
            lista[0].rotulo, "fedora",
            "o nome anunciado vence o rÃ³tulo genÃ©rico"
        );
    }

    #[test]
    fn o_id_anunciado_e_o_mesmo_texto_dos_dois_lados() {
        assert_eq!(id_de(MachineId([0xab; 16])), "ab".repeat(16));
    }
}
