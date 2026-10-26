//! O serviço atendendo o painel no Android ([12, §3](../../../docs/12-painel-android.md)).
//!
//! A conversa em si — Noise, código, quadros cifrados — é do [`ir_monitor`], sem E/S. Aqui fica o
//! que é do serviço: a porta TCP, uma tarefa por painel conectado, a janela de pareamento e a
//! distribuição do quadro de cada segundo.
//!
//! # Quem é dono de quê
//!
//! Uma **central** é a única dona do estado: os painéis conhecidos, a janela, quem está pareando,
//! quem assina. As tarefas de cada conexão só falam com ela por canal ([02, §4]). O serviço manda
//! [`Comando`]s e recebe [`Aviso`]s; o quadro e os detalhes vão por [`Monitor::publicar`],
//! codificados uma vez e cifrados por cada conexão. Os detalhes só vão a quem disse que os quer.
//!
//! # Custo
//!
//! - A porta só fica aberta com um painel pareado, com a janela de pareamento aberta, ou com os
//!   pedidos de aparelhos novos aceitos ([`Comando::AceitarPedidos`]). Um endereço que insiste
//!   depois de recusado fica de fora por um tempo (`recusas`).
//! - A assinatura de um painel **expira** em [`ASSINATURA_EXPIRA`] sem renovação: um tablet que
//!   sumiu da rede sem avisar não deixa a coleta ligada. Sem assinante, o serviço não coleta nada
//!   ([`Aviso::Assinantes`]).
//! - Os processos só são medidos quando um painel assinante os quer ([`Aviso::Interesse`]).
//!
//! [02, §4]: ../../../docs/02-arquitetura.md

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

mod central;
mod conexao;
mod recusas;

use std::sync::Arc;
use std::time::Duration;

use ir_crypto::{Identity, PublicKey};
use ir_monitor::mensagem::{self, ParaPainel};
use ir_monitor::{Detalhes, InteresseEmProcessos, Quadro};
use tokio::sync::{mpsc, watch};

/// Quanto tempo a janela de pareamento fica aberta: os mesmos três minutos do pareamento entre os
/// computadores ([04, §3](../../../docs/04-seguranca.md)).
pub const JANELA_DE_PAREAMENTO: Duration = Duration::from_secs(180);

/// Quanto tempo o código de um pareamento espera a resposta das duas telas. Sem resposta, o
/// pedido é recusado: com o pedido partindo do aparelho, ninguém no computador pediu nada, e o
/// código não pode ficar na tela para sempre.
pub const PRAZO_DO_CODIGO: Duration = Duration::from_secs(60);

/// Depois de quanto tempo sem renovação uma assinatura vale como desfeita. O painel renova a cada
/// 2 s; cinco é folga para uma renovação perdida no Wi-Fi.
pub const ASSINATURA_EXPIRA: Duration = Duration::from_secs(5);

/// O que o serviço pede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comando {
    /// Abra a janela de pareamento por [`JANELA_DE_PAREAMENTO`].
    AbrirPareamento,
    /// Feche a janela, e desista do pareamento em curso.
    FecharPareamento,
    /// A pessoa respondeu ao código: `true` se é o mesmo do painel.
    Confirmar(bool),
    /// Os painéis pareados, como estão gravados. Quem sair da lista é desconectado na hora.
    Conhecidos(Vec<PublicKey>),
    /// Aceite (`true`) que um aparelho novo peça para parear sem a janela aberta: o código aparece
    /// sozinho no computador, e a pessoa confere nas duas telas. Desligado, só pela janela.
    AceitarPedidos(bool),
}

/// O que o monitor conta ao serviço.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aviso {
    /// Um painel novo chegou ao código: mostre-o na janela.
    Codigo([u8; 6]),
    /// O pareamento deu certo: grave a chave.
    Pareado(PublicKey),
    /// Um painel conectado disse o nome dele.
    Apresentou {
        /// Qual.
        chave: PublicKey,
        /// O nome do aparelho.
        nome: String,
    },
    /// O pareamento em curso não chegou ao fim: recusado, prazo, ou o painel saiu.
    PareamentoFalhou,
    /// Quantos painéis estão assinando o quadro agora. De zero para um, o serviço começa a
    /// coletar; de volta a zero, para.
    Assinantes(usize),
    /// Os painéis conectados agora, para a janela mostrar quem está ligado.
    Conectados(Vec<PublicKey>),
    /// O que os painéis assinantes querem dos processos de cada computador: este (0) e o par (1).
    /// Vem quando muda.
    Interesse([InteresseEmProcessos; 2]),
}

/// O que vai a cada segundo, já em bytes: o quadro para todos, os detalhes para quem os quer.
#[derive(Debug)]
pub(crate) struct Publicacao {
    pub(crate) quadro: Vec<u8>,
    pub(crate) detalhes: Option<Vec<u8>>,
}

/// O monitor em funcionamento. Soltá-lo fecha a porta e todas as conexões.
#[derive(Debug)]
pub struct Monitor {
    comandos: mpsc::UnboundedSender<Comando>,
    quadro: watch::Sender<Option<Arc<Publicacao>>>,
}

impl Monitor {
    /// Começa o monitor deste computador. `nome` é como o painel o mostra.
    #[must_use]
    pub fn iniciar(
        identidade: Identity,
        nome: &str,
        conhecidos: Vec<PublicKey>,
        avisos: mpsc::UnboundedSender<Aviso>,
    ) -> Self {
        Self::iniciar_na_porta(identidade, nome, conhecidos, avisos, ir_monitor::PORTA)
    }

    /// Como [`Self::iniciar`], noutra porta — para teste.
    #[must_use]
    pub fn iniciar_na_porta(
        identidade: Identity,
        nome: &str,
        conhecidos: Vec<PublicKey>,
        avisos: mpsc::UnboundedSender<Aviso>,
        porta: u16,
    ) -> Self {
        let (comandos, recebidos) = mpsc::unbounded_channel();
        let (quadro, quadros) = watch::channel(None);
        let central = central::Central::nova(central::Partes {
            identidade,
            nome: nome.into(),
            conhecidos,
            avisos,
            quadros,
            porta,
        });
        tokio::spawn(central.rodar(recebidos));
        Self { comandos, quadro }
    }

    /// Um pedido do serviço.
    pub fn comandar(&self, comando: Comando) {
        let _ = self.comandos.send(comando);
    }

    /// O quadro e os detalhes deste segundo, para quem assina. Codificados aqui, uma vez para
    /// todos.
    pub fn publicar(&self, quadro: Quadro, detalhes: Detalhes) {
        let Ok(quadro) = mensagem::codificar(&ParaPainel::Quadro(quadro)) else {
            return;
        };
        let detalhes = mensagem::codificar(&ParaPainel::Detalhes(detalhes)).ok();
        self.quadro
            .send_replace(Some(Arc::new(Publicacao { quadro, detalhes })));
    }
}
