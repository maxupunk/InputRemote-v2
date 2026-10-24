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
//! [`Comando`]s e recebe [`Aviso`]s; o quadro vai por [`Monitor::publicar`], codificado uma vez
//! e cifrado por cada conexão.
//!
//! # Custo
//!
//! - A porta só fica aberta com um painel pareado ou com a janela de pareamento aberta.
//! - A assinatura de um painel **expira** em [`ASSINATURA_EXPIRA`] sem renovação: um tablet que
//!   sumiu da rede sem avisar não deixa a coleta ligada. Sem assinante, o serviço não coleta nada
//!   ([`Aviso::Assinantes`]).
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

use std::sync::Arc;
use std::time::Duration;

use ir_crypto::{Identity, PublicKey};
use ir_monitor::Quadro;
use ir_monitor::mensagem::{self, ParaPainel};
use tokio::sync::{mpsc, watch};

/// Quanto tempo a janela de pareamento fica aberta: os mesmos três minutos do pareamento entre os
/// computadores ([04, §3](../../../docs/04-seguranca.md)).
pub const JANELA_DE_PAREAMENTO: Duration = Duration::from_secs(180);

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
}

/// O monitor em funcionamento. Soltá-lo fecha a porta e todas as conexões.
#[derive(Debug)]
pub struct Monitor {
    comandos: mpsc::UnboundedSender<Comando>,
    quadro: watch::Sender<Option<Arc<Vec<u8>>>>,
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

    /// O quadro deste segundo, para quem assina. Codificado aqui, uma vez para todos.
    pub fn publicar(&self, quadro: Quadro) {
        if let Ok(bytes) = mensagem::codificar(&ParaPainel::Quadro(quadro)) {
            self.quadro.send_replace(Some(Arc::new(bytes)));
        }
    }
}
