//! A faixa da pasta compartilhada no canal de arquivos
//! ([ADR-0015](../../../docs/adr/0015-pastas-compartilhadas.md)).
//!
//! As mensagens da pasta usam o mesmo enlace TCP da cópia do clipboard — a mesma descoberta, a
//! mesma reconexão quando o IP muda —, mas **não** a fila dela: lá uma cópia nova cancela a
//! anterior, e a sincronia não pode cancelar o Ctrl+C do usuário nem ser cancelada por ele. Daqui
//! elas só passam: quem as entende é o ajudante das pastas, do outro lado do canal local.
//!
//! Duas regras moram aqui porque é aqui que o fio está:
//!
//! - **O portão de versão.** Mensagem desconhecida derruba o enlace (`docs/03-protocolo.md` §8): uma
//!   mensagem da pasta mandada a um par da versão 7 derrubaria o canal inteiro, com a cópia que
//!   estivesse passando. Sem o par ter negociado a 8, nada sai.
//! - **Sem enlace, descarta.** O ajudante sabe que o canal caiu ([`EstadoDaFaixa`]) e manda de novo
//!   quando ele voltar; guardar aqui só atrasaria a mensagem nova atrás de uma velha.

use std::sync::{Arc, Mutex as Trava};

use ir_proto::message::{BulkMessage, FolderMessage, validate_folder_message};
use ir_transporte::dados::Remetente;
use tokio::sync::{Mutex, mpsc, watch};
use tracing::{debug, warn};

/// Quantas mensagens do par esperam o ajudante antes de a leitura do canal parar.
///
/// A contrapressão é a certa: o par só manda trecho que a réplica pediu, até 4 MiB por pedido.
const FILA_DE_CHEGADA: usize = 256;

/// Quantas mensagens do ajudante esperam a vez de sair.
const FILA_DE_SAIDA: usize = 256;

/// Como o canal de dados está, para o ajudante das pastas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EstadoDaFaixa {
    /// Se o canal de dados está de pé.
    pub de_pe: bool,
    /// Se o par negociou uma versão que conhece pastas.
    pub par_suporta: bool,
    /// Se a versão do par já é conhecida. Antes da primeira sessão de entrada ela não é, e "não
    /// sei" não pode virar "o outro computador está desatualizado" na tela.
    pub versao_conhecida: bool,
    /// O nome do outro computador.
    pub nome_do_par: String,
}

/// O lado do serviço que fala com o ajudante: por onde mandar ao par, por onde chega do par, e como
/// o canal está.
#[derive(Debug)]
pub struct Faixa {
    /// Para o par. Sai se o canal está de pé e o par conhece pastas; senão, é descartada.
    pub para_o_par: mpsc::Sender<FolderMessage>,
    /// Do par, já conferidas.
    pub do_par: mpsc::Receiver<FolderMessage>,
    /// Como o canal está.
    pub estado: watch::Receiver<EstadoDaFaixa>,
    /// Copiar e colar: o que o outro computador copiou de uma pasta só vai ao clipboard ligada.
    pub copias: crate::ChaveDaCopia,
}

/// O lado da transferência: o que o canal precisa para desviar e despachar.
#[derive(Debug, Clone)]
pub(crate) struct Desvio {
    chegada: mpsc::Sender<FolderMessage>,
    estado: Arc<watch::Sender<EstadoDaFaixa>>,
    /// O remetente do enlace de agora, quando há um.
    remetente: Arc<Trava<Option<Arc<Mutex<Remetente>>>>>,
}

impl Desvio {
    /// A faixa e o desvio, ligados. Sobe a tarefa que despacha o que o ajudante manda.
    pub(crate) fn novo(copias: crate::ChaveDaCopia) -> (Self, Faixa) {
        let (chegada, do_par) = mpsc::channel(FILA_DE_CHEGADA);
        let (saida, para_despachar) = mpsc::channel(FILA_DE_SAIDA);
        let (estado, recebe_estado) = watch::channel(EstadoDaFaixa::default());
        let desvio = Self {
            chegada,
            estado: Arc::new(estado),
            remetente: Arc::default(),
        };
        tokio::spawn(despachar(desvio.clone(), para_despachar));
        let faixa = Faixa {
            para_o_par: saida,
            do_par,
            estado: recebe_estado,
            copias,
        };
        (desvio, faixa)
    }

    /// Um desvio sem ninguém do outro lado, para a alça desligada e para teste.
    pub(crate) fn solto() -> Self {
        let (chegada, _) = mpsc::channel(1);
        let (estado, _) = watch::channel(EstadoDaFaixa::default());
        Self {
            chegada,
            estado: Arc::new(estado),
            remetente: Arc::default(),
        }
    }

    /// O enlace subiu, com este remetente; ou caiu, com `None`.
    pub(crate) fn enlace(&self, remetente: Option<Arc<Mutex<Remetente>>>) {
        let de_pe = remetente.is_some();
        if let Ok(mut atual) = self.remetente.lock() {
            *atual = remetente;
        }
        self.estado.send_if_modified(|estado| {
            let mudou = estado.de_pe != de_pe;
            estado.de_pe = de_pe;
            mudou
        });
    }

    /// O que se sabe do par agora: se ele conhece pastas, e o nome dele.
    pub(crate) fn par(&self, suporta: bool, nome: &str) {
        self.estado.send_if_modified(|estado| {
            let mudou = estado.par_suporta != suporta
                || estado.nome_do_par != nome
                || !estado.versao_conhecida;
            estado.par_suporta = suporta;
            estado.versao_conhecida = true;
            nome.clone_into(&mut estado.nome_do_par);
            mudou
        });
    }

    /// Uma mensagem da pasta chegou do par. `false` quando ela não vale e o enlace deve cair:
    /// o par mandou o que a validação recusa, e agir sob discordância é o que o protocolo proíbe.
    pub(crate) async fn chegou(&self, mensagem: FolderMessage) -> bool {
        if let Err(erro) = validate_folder_message(&mensagem) {
            warn!(%erro, "mensagem da pasta inválida; o canal de arquivos cai");
            return false;
        }
        if self.chegada.send(mensagem).await.is_err() {
            debug!("ninguém ouvindo a pasta compartilhada; mensagem descartada");
        }
        true
    }

    fn remetente(&self) -> Option<Arc<Mutex<Remetente>>> {
        self.remetente.lock().ok().and_then(|atual| atual.clone())
    }

    fn portao_aberto(&self) -> bool {
        let estado = self.estado.borrow();
        estado.de_pe && estado.par_suporta
    }
}

/// Tira da fila o que o ajudante manda e põe no enlace, enquanto o serviço viver.
async fn despachar(desvio: Desvio, mut fila: mpsc::Receiver<FolderMessage>) {
    while let Some(mensagem) = fila.recv().await {
        if !desvio.portao_aberto() {
            debug!("pasta: sem enlace ou par sem suporte; mensagem descartada");
            continue;
        }
        let Some(remetente) = desvio.remetente() else {
            continue;
        };
        let resultado = remetente
            .lock()
            .await
            .enviar_agora(BulkMessage::Folder(mensagem))
            .await;
        if let Err(falha) = resultado {
            // Quem detecta a queda é quem lê; aqui só não adianta insistir.
            debug!(%falha, "pasta: a mensagem não saiu");
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[tokio::test]
    async fn sem_o_par_negociar_pastas_o_portao_fica_fechado() {
        let desvio = Desvio::solto();
        assert!(!desvio.portao_aberto(), "sem enlace e sem par");
        desvio.par(true, "NOTEBOOK");
        assert!(!desvio.portao_aberto(), "o par conhece, mas não há enlace");
        let estado = desvio.estado.borrow().clone();
        assert_eq!(estado.nome_do_par, "NOTEBOOK");
        desvio.par(false, "NOTEBOOK");
        assert!(
            !desvio.estado.borrow().par_suporta,
            "a versão 7 fecha o portão"
        );
    }

    #[tokio::test]
    async fn uma_mensagem_invalida_do_par_derruba_o_enlace() {
        let (desvio, mut faixa) = Desvio::novo(crate::ChaveDaCopia::default());
        let ruim = FolderMessage::Delete {
            folder: ir_proto::message::FolderId([1; 16]),
            op: ir_proto::message::OpId(1),
            path: "../fora".to_owned(),
            base: 1,
        };
        assert!(!desvio.chegou(ruim).await);
        let boa = FolderMessage::HelperAbsent;
        assert!(desvio.chegou(boa.clone()).await);
        assert_eq!(faixa.do_par.recv().await, Some(boa));
    }
}
