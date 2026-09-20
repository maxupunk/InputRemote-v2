//! Quem está por perto para parear: a rede (a pergunta por broadcast do `ir-net`), o rádio (pareados
//! no sistema) e o endereço que alguém escreveu na configuração.
//!
//! Antes, "Procurar" devolvia só o `peer_addr` do arquivo de configuração. Numa instalação limpa ele
//! não existe, e a lista vinha vazia; com um que sobrou de outra rede, ela mostrava um computador que
//! não estava lá. A lista de pareados do sistema já existia no `ir-bt`, e ninguém a chamava; a
//! pergunta na rede nasceu junto com este módulo.
//!
//! Este módulo mora aqui, e não no serviço, pela mesma razão do resto do crate: é o único lugar que
//! conhece a rede e o rádio ao mesmo tempo ([02, §2](../../../docs/02-arquitetura.md)).

use std::net::SocketAddr;
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
/// é para o Wi-Fi em economia de energia, que atrasa broadcast.
const DURACAO_DA_BUSCA: Duration = Duration::from_secs(2);

/// A descoberta desta máquina: responde a quem pergunta, e pergunta pelos outros.
#[derive(Debug, Clone)]
pub struct Descoberta {
    propria: String,
    pareados: Option<Pareados>,
}

impl Descoberta {
    /// Uma descoberta desta máquina, ainda sem anúncio. `pareados` é a alça do rádio, quando há.
    #[must_use]
    pub fn nova(maquina: MachineId, pareados: Option<Pareados>) -> Self {
        Self {
            propria: id_de(maquina),
            pareados,
        }
    }

    /// Uma descoberta que não acha nada: para a bancada de testes do serviço.
    #[must_use]
    pub fn desligada() -> Self {
        Self {
            propria: String::new(),
            pareados: None,
        }
    }

    /// Passa a responder a quem perguntar na rede, com o nome desta máquina e a porta do serviço.
    ///
    /// # Errors
    ///
    /// Se a porta da descoberta não abrir. Os outros não acham esta máquina sozinhos, mas o rádio e
    /// o endereço digitado continuam funcionando, e esta ainda acha as outras.
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

    /// Procura na rede e no rádio, e junta com o endereço configurado.
    ///
    /// Devolve um futuro que não depende de `self`, para rodar numa tarefa própria: a busca leva
    /// segundos, e o ator do serviço não pode esperar nada. Nunca falha: uma fonte que não responde
    /// só não contribui. A lista vazia é uma resposta — e a janela diz o que fazer com ela.
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

impl Descoberta {
    /// Onde está, na rede local, a máquina com este id — para quem só a conhece pelo rádio.
    ///
    /// Como [`Self::procurar`], não depende de `self` e nunca falha: ninguém respondendo é `None`.
    pub fn localizar(
        &self,
        maquina: MachineId,
    ) -> impl Future<Output = Option<SocketAddr>> + Send + 'static {
        let procurada = id_de(maquina);
        async move {
            let rede = ir_net::descoberta::procurar(DURACAO_DA_BUSCA)
                .await
                .unwrap_or_default();
            endereco_de(&procurada, rede)
        }
    }
}

/// O endereço de quem se anunciou com este id.
fn endereco_de(maquina: &str, rede: Vec<Candidate>) -> Option<SocketAddr> {
    rede.into_iter()
        .find(|candidato| candidato.machine == maquina)
        .map(|candidato| candidato.addr)
}

/// O identificador anunciado, em hexadecimal: o mesmo texto dos dois lados, para esta máquina se
/// reconhecer na própria busca.
fn id_de(maquina: MachineId) -> String {
    use std::fmt::Write as _;
    maquina.0.iter().fold(String::new(), |mut texto, byte| {
        let _ = write!(texto, "{byte:02x}");
        texto
    })
}

/// Junta as fontes numa lista só, na ordem em que vale a pena tentar.
///
/// Rede primeiro: quem responde à pergunta é o InputRemote, com certeza. Depois o rádio: a lista de
/// pareados do sistema traz também fones, alto-falantes e teclados, e deles só ficam os que se
/// declaram computador ([`Dispositivo::e_computador`]). Por último o endereço da configuração, marcado como tal — pode ser de
/// outra rede. Esta própria máquina e endereços repetidos saem.
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
    for dispositivo in pareados.into_iter().filter(Dispositivo::e_computador) {
        acrescentar(Encontrado {
            rotulo: dispositivo.nome,
            endereco: Endereco::Radio(dispositivo.endereco),
        });
    }
    if let Some(endereco) = configurado {
        acrescentar(Encontrado {
            rotulo: format!("Endereço configurado: {endereco}"),
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

    /// Classes reais, lidas de `/var/lib/bluetooth` e do Windows na bancada.
    const NOTEBOOK: u32 = 0x001c_010c;
    const FONE: u32 = 0x0024_0404;
    const ALTO_FALANTE: u32 = 0x0024_0414;
    const TECLADO: u32 = 0x0000_2540;
    const TELEFONE: u32 = 0x005a_020c;

    fn pareado(nome: &str, byte: u8) -> Dispositivo {
        pareado_da_classe(nome, byte, NOTEBOOK)
    }

    fn pareado_da_classe(nome: &str, byte: u8, classe: u32) -> Dispositivo {
        Dispositivo {
            endereco: BdAddr([byte; 6]),
            nome: nome.to_owned(),
            conectado: false,
            classe,
        }
    }

    #[test]
    fn do_radio_so_aparecem_computadores() {
        let lista = juntar(
            "aaaa",
            Vec::new(),
            vec![
                pareado_da_classe("Buds2 Pro", 1, FONE),
                pareado_da_classe("Echo Dot", 2, ALTO_FALANTE),
                pareado_da_classe("MCHOSE V9 PRO", 3, TECLADO),
                pareado_da_classe("Galaxy", 4, TELEFONE),
                pareado_da_classe("sem classe", 5, 0),
                pareado_da_classe("fedora", 6, NOTEBOOK),
            ],
            None,
        );
        let rotulos: Vec<&str> = lista.iter().map(|e| e.rotulo.as_str()).collect();
        assert_eq!(rotulos, ["fedora"]);
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
                "Endereço configurado: 10.0.0.99:52525"
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
            "o nome anunciado vence o rótulo genérico"
        );
    }

    #[test]
    fn localizar_devolve_so_a_maquina_procurada() {
        let rede = vec![
            na_rede("outro", "cccc", "10.0.0.7:52525"),
            na_rede("fedora", "bbbb", "10.0.0.135:52525"),
        ];
        assert_eq!(
            endereco_de("bbbb", rede.clone()),
            "10.0.0.135:52525".parse().ok()
        );
        assert_eq!(endereco_de("dddd", rede), None);
    }

    #[test]
    fn o_id_anunciado_e_o_mesmo_texto_dos_dois_lados() {
        assert_eq!(id_de(MachineId([0xab; 16])), "ab".repeat(16));
    }
}
