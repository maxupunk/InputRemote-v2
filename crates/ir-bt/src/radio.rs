//! O rádio: a costura entre o protocolo e a pilha Bluetooth de cada sistema.
//!
//! Tudo que este crate tem de protocolo — enquadramento, handshake, enlace cifrado, e a máquina
//! de estados que vem em cima deles — trabalha contra [`Radio`] e [`Canal`](crate::Canal), e não
//! contra Winsock nem BlueZ. Os backends implementam; nada acima deles sabe qual está embaixo.
//!
//! É a inversão de dependência de [09, §6](../../../docs/09-padroes-de-codigo.md) aplicada onde
//! ela paga mais: o hardware é a única parte que não dá para rodar no CI, então é exatamente a
//! parte que precisa ser substituível. Sem esta fronteira, testar o pareamento exigiria dois
//! computadores e um rádio — foi assim que o v1 chegou ao fim sem um único teste de Bluetooth
//! ([00, §6](../../../docs/00-licoes-do-v1.md)).
//!
//! # O que o produto faz, e o que ele não faz
//!
//! Não há `parear` neste *trait*, e a ausência é a decisão: o pareamento do **sistema** é do
//! usuário, pelas configurações do próprio sistema operacional
//! ([ADR-0005](../../../docs/adr/0005-bluetooth-rfcomm-winsock.md), Decisão B). O produto
//! enxerga quem já está pareado ([`Radio::pareados`]), conecta, e explica quando falta parear.
//! O código de seis dígitos do InputRemote é outra camada, e é nossa.

use core::future::Future;

use crate::addr::BdAddr;
use crate::canal::Canal;
use crate::error::Result;

/// Um computador pareado no sistema, como o rádio o enxerga.
///
/// O nome vem do sistema operacional, que é onde o usuário o escolheu ao parear. Repeti-lo aqui
/// é o que permite a tela mostrar "notebook do Maxuel" em vez de `AC:50:DE:47:EB:28`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispositivo {
    /// O endereço do rádio dele.
    pub endereco: BdAddr,
    /// Como o sistema o chama.
    pub nome: String,
    /// Se o rádio está ligado a ele agora.
    ///
    /// Pareado e conectado são coisas diferentes: um par pareado que não está conectado continua
    /// pareado, e o sistema liga sozinho quando alguém abre um canal.
    pub conectado: bool,
    /// A classe do dispositivo (*Class of Device*), como o sistema a guardou; zero se ele não disse.
    pub classe: u32,
}

/// A classe maior "computador", nos bits 8 a 12 da classe do dispositivo (Bluetooth Assigned
/// Numbers, *Baseband*). Notebook, desktop e servidor são todos ela.
const CLASSE_MAIOR_COMPUTADOR: u32 = 0x01;

impl Dispositivo {
    /// Se é um computador — o único tipo de dispositivo que pode ter o InputRemote.
    ///
    /// A lista de pareados do sistema traz fones, alto-falantes, teclados e relógios junto; na tela
    /// de "Parear" eles só confundem, e escolher um dá erro. Classe desconhecida conta como "não":
    /// a rede e o endereço digitado continuam alcançando um computador que não a declara.
    #[must_use]
    pub const fn e_computador(&self) -> bool {
        (self.classe >> 8) & 0x1F == CLASSE_MAIOR_COMPUTADOR
    }
}

/// O rádio Bluetooth desta máquina.
///
/// Uma implementação por sistema operacional, e uma de mentira nos testes. Os métodos são
/// `impl Future + Send` em vez de `async fn` de propósito: o futuro precisa ser `Send` para as
/// tarefas do serviço poderem carregá-lo entre threads, e escrever isso à mão é o que garante.
pub trait Radio: Send + Sync + 'static {
    /// O tipo de canal que este rádio produz.
    type Canal: Canal + 'static;

    /// Se há rádio utilizável nesta máquina agora.
    ///
    /// Falso quando não há adaptador, ele está desligado, ou o sistema o bloqueou. É o que
    /// decide entre tentar o Bluetooth e degradar para a rede com o motivo dito na tela — a
    /// política em si é do `ir-session`, não daqui.
    fn disponivel(&self) -> impl Future<Output = bool> + Send;

    /// O endereço do rádio desta máquina, se ele disser.
    ///
    /// Síncrono de propósito: nos dois sistemas é uma consulta local e imediata, e quem pergunta
    /// é a subida do serviço. Serve para a sessão contar ao par por onde discar o Bluetooth quando
    /// os dois se conheceram pela rede ([ADR-0012](../../../docs/adr/0012-rota-dupla.md)).
    /// `None` não é falha: o par ainda pode discar para cá se já souber o endereço, e a rede
    /// continua funcionando sozinha.
    fn endereco_local(&self) -> Option<BdAddr>;

    /// Os computadores já pareados neste sistema.
    ///
    /// # Errors
    ///
    /// [`BtError::SemRadio`](crate::BtError::SemRadio) se não houver rádio utilizável.
    fn pareados(&self) -> impl Future<Output = Result<Vec<Dispositivo>>> + Send;

    /// Abre um canal com o par, no canal RFCOMM do produto.
    ///
    /// # Errors
    ///
    /// [`BtError::NaoPareado`](crate::BtError::NaoPareado) se o par não estiver pareado no
    /// sistema; [`BtError::SemResposta`](crate::BtError::SemResposta) se estiver pareado mas
    /// ninguém atender. A distinção é obrigatória: são os dois lados da pergunta "por que não
    /// conecta?", e confundi-los manda o usuário mexer no lugar errado.
    fn conectar(&self, alvo: BdAddr) -> impl Future<Output = Result<Self::Canal>> + Send;

    /// Espera o par abrir um canal com esta máquina.
    ///
    /// Devolve o canal e de quem ele veio.
    ///
    /// # Errors
    ///
    /// [`BtError::Io`](crate::BtError::Io) se a escuta falhar.
    fn aceitar(&self) -> impl Future<Output = Result<(Self::Canal, BdAddr)>> + Send;
}

/// Dois rádios ligados um no outro, para os testes.
///
/// O "outro computador" é um canal em memória. É o que permite provar o pareamento inteiro — o
/// código de seis dígitos, as duas confirmações, o quadro que só passa depois delas — num teste
/// que roda em milissegundos, sem dois computadores e sem rádio.
#[cfg(test)]
pub(crate) mod mentira {
    use std::sync::Arc;

    use tokio::io::DuplexStream;
    use tokio::sync::{Mutex, mpsc};

    use super::{Dispositivo, Radio};
    use crate::addr::BdAddr;
    use crate::error::{BtError, Result};

    /// O endereço da primeira máquina de teste.
    pub(crate) const AQUI: BdAddr = BdAddr([0x74, 0x13, 0xEA, 0xA6, 0x5A, 0x99]);
    /// O endereço da segunda.
    pub(crate) const LA: BdAddr = BdAddr([0xAC, 0x50, 0xDE, 0x47, 0xEB, 0x28]);

    /// Um canal entrante: por onde ele chega, e de quem.
    type Entrante = (DuplexStream, BdAddr);

    /// Um rádio de mentira.
    #[derive(Debug)]
    pub(crate) struct RadioDeMentira {
        endereco: BdAddr,
        /// Para onde vai a outra ponta de cada canal que este rádio abre.
        para_o_outro: mpsc::UnboundedSender<Entrante>,
        /// De onde vêm os canais que o outro abriu conosco.
        entradas: Mutex<mpsc::UnboundedReceiver<Entrante>>,
        /// Quem está pareado no "sistema" desta máquina.
        pareados: Vec<BdAddr>,
        disponivel: bool,
        /// Se a escuta morreu, como no Windows quando o Bluetooth é desligado.
        escuta_morta: bool,
    }

    impl RadioDeMentira {
        /// Dois rádios ligados um no outro, ambos pareados.
        pub(crate) fn par() -> (Arc<Self>, Arc<Self>) {
            Self::com_pareamento(true)
        }

        /// Dois rádios ligados, mas que não se conhecem: conectar dá
        /// [`BtError::NaoPareado`].
        pub(crate) fn sem_pareamento() -> (Arc<Self>, Arc<Self>) {
            Self::com_pareamento(false)
        }

        fn com_pareamento(pareados: bool) -> (Arc<Self>, Arc<Self>) {
            let (para_aqui, de_la) = mpsc::unbounded_channel();
            let (para_la, de_aqui) = mpsc::unbounded_channel();
            let lista = |outro| if pareados { vec![outro] } else { Vec::new() };
            (
                Arc::new(Self {
                    endereco: AQUI,
                    para_o_outro: para_la,
                    entradas: Mutex::new(de_la),
                    pareados: lista(LA),
                    disponivel: true,
                    escuta_morta: false,
                }),
                Arc::new(Self {
                    endereco: LA,
                    para_o_outro: para_aqui,
                    entradas: Mutex::new(de_aqui),
                    pareados: lista(AQUI),
                    disponivel: true,
                    escuta_morta: false,
                }),
            )
        }

        /// Um rádio que não existe nesta máquina.
        pub(crate) fn desligado() -> Arc<Self> {
            let (para_o_outro, entradas) = mpsc::unbounded_channel();
            Arc::new(Self {
                endereco: AQUI,
                para_o_outro,
                entradas: Mutex::new(entradas),
                pareados: Vec::new(),
                disponivel: false,
                escuta_morta: false,
            })
        }

        /// Um rádio que existia e foi desligado: a escuta acabou.
        pub(crate) fn desligado_depois() -> Arc<Self> {
            let (para_o_outro, entradas) = mpsc::unbounded_channel();
            Arc::new(Self {
                endereco: AQUI,
                para_o_outro,
                entradas: Mutex::new(entradas),
                pareados: Vec::new(),
                disponivel: true,
                escuta_morta: true,
            })
        }
    }

    impl Radio for RadioDeMentira {
        type Canal = DuplexStream;

        async fn disponivel(&self) -> bool {
            self.disponivel
        }

        fn endereco_local(&self) -> Option<BdAddr> {
            self.disponivel.then_some(self.endereco)
        }

        async fn pareados(&self) -> Result<Vec<Dispositivo>> {
            if !self.disponivel {
                return Err(BtError::SemRadio("rádio de mentira desligado".to_owned()));
            }
            Ok(self
                .pareados
                .iter()
                .map(|endereco| Dispositivo {
                    endereco: *endereco,
                    nome: format!("máquina {endereco}"),
                    conectado: false,
                    classe: 0x0001_010C,
                })
                .collect())
        }

        async fn conectar(&self, alvo: BdAddr) -> Result<Self::Canal> {
            if !self.disponivel {
                return Err(BtError::SemRadio("rádio de mentira desligado".to_owned()));
            }
            if !self.pareados.contains(&alvo) {
                return Err(BtError::NaoPareado(alvo.to_string()));
            }
            let (minha, dele) = tokio::io::duplex(16 * 1024);
            self.para_o_outro
                .send((dele, self.endereco))
                .map_err(|_| BtError::SemResposta)?;
            Ok(minha)
        }

        async fn aceitar(&self) -> Result<(Self::Canal, BdAddr)> {
            if self.escuta_morta {
                return Err(BtError::SemRadio(
                    "o rádio de mentira foi desligado".to_owned(),
                ));
            }
            let mut entradas = self.entradas.lock().await;
            match entradas.recv().await {
                Some(entrante) => Ok(entrante),
                // Ninguém mais vai ligar. Devolver erro aqui faria o laço do endpoint girar sem
                // parar; esperar para sempre é o comportamento certo de uma escuta sem ninguém.
                None => std::future::pending().await,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mentira::{LA, RadioDeMentira};
    use super::*;
    use crate::error::BtError;

    fn da_classe(classe: u32) -> Dispositivo {
        Dispositivo {
            endereco: BdAddr([1; 6]),
            nome: String::new(),
            conectado: false,
            classe,
        }
    }

    #[test]
    fn computador_e_a_classe_maior_1_e_nada_mais() {
        // Notebook (Fedora na bancada), desktop, servidor: classe maior 0x01, qualquer menor.
        for classe in [0x001c_010c, 0x0000_0104, 0x0000_0108] {
            assert!(da_classe(classe).e_computador(), "{classe:#08x}");
        }
        // Telefone, fone, alto-falante, teclado, mouse, relógio, "sem categoria" e desconhecida.
        for classe in [
            0x005a_020c,
            0x0024_0404,
            0x0024_0414,
            0x0000_2540,
            0x0000_2580,
            0x0000_0704,
            0x0000_1f00,
            0,
        ] {
            assert!(!da_classe(classe).e_computador(), "{classe:#08x}");
        }
    }

    #[tokio::test]
    async fn um_radio_desligado_diz_que_esta_desligado_em_vez_de_falhar_por_outro_motivo() {
        // É o caminho da degradação para a rede: o produto precisa saber que **não há rádio**,
        // e não descobrir isso como um erro genérico de socket. É essa distinção que permite
        // dizer "Bluetooth indisponível; usando a rede local" com honestidade.
        let radio = RadioDeMentira::desligado();
        assert!(!radio.disponivel().await);

        let erro = radio.pareados().await.expect_err("sem rádio não há lista");
        assert!(matches!(erro, BtError::SemRadio(_)), "{erro}");
        let instrucao = erro.o_que_fazer().expect("há o que fazer");
        assert!(instrucao.contains("Ligue o Bluetooth"), "{instrucao}");
    }

    #[test]
    fn o_radio_sabe_o_proprio_endereco_e_o_desligado_nao_inventa_um() {
        let (aqui, _la) = RadioDeMentira::par();
        assert_eq!(aqui.endereco_local(), Some(super::mentira::AQUI));
        assert_eq!(RadioDeMentira::desligado().endereco_local(), None);
    }

    #[tokio::test]
    async fn um_radio_desligado_nao_tenta_conectar() {
        let radio = RadioDeMentira::desligado();
        let erro = radio.conectar(LA).await.expect_err("sem rádio não conecta");
        assert!(matches!(erro, BtError::SemRadio(_)), "{erro}");
    }

    #[tokio::test]
    async fn dois_radios_pareados_se_enxergam_com_nome_e_endereco() {
        // O que a tela "Procurar" vai listar. Sem o nome, o usuário escolheria entre endereços
        // hexadecimais — que é o mesmo que não oferecer escolha.
        let (aqui, _la) = RadioDeMentira::par();
        let pareados = aqui.pareados().await.expect("há rádio");
        assert_eq!(pareados.len(), 1);
        let dispositivo = pareados.first().expect("um dispositivo");
        assert_eq!(dispositivo.endereco, LA);
        assert!(!dispositivo.nome.is_empty());
    }
}
