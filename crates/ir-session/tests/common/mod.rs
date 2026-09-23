//! Bancada de duas sessões conversando.
//!
//! Este arquivo é a prova prática do argumento de [ADR-0004]: duas máquinas, um enlace, um
//! relógio — tudo em memória, determinístico, em microssegundos. Não há socket, não há rádio,
//! não há segundo computador, e mesmo assim os cenários exercidos são os reais.
//!
//! O roteamento é a única mágica: todo `Command::Send` de um lado vira `Input::Received` do
//! outro, em cascata, até a conversa se acalmar. É o que uma rede sem perda faria.
//!
//! [ADR-0004]: ../../../docs/adr/0004-nucleo-sans-io.md

#![allow(
    dead_code,
    unreachable_pub,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic
)]

use ir_proto::carrier::Carrier;
use ir_proto::ids::{MachineId, MonitorId};
use ir_proto::peer::{Capabilities, ClipboardCapabilities, MachineName, PrivilegedInputLevel};
use ir_proto::screens::{Edge, MonitorInfo, ScreenLayout};
use ir_session::event::Notice;
use ir_session::{Command, CommandBatch, Input, LocalIdentity, Session, SessionConfig, Timestamp};

/// Quem é quem, para o roteamento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Server,
    Client,
}

impl Side {
    pub const fn other(self) -> Self {
        match self {
            Self::Server => Self::Client,
            Self::Client => Self::Server,
        }
    }
}

/// Duas sessões, um relógio e um enlace sem perda.
pub struct Pair {
    pub server: Session,
    pub client: Session,
    now: Timestamp,
    /// Tudo que os dois lados pediram, em ordem, desde o último `take`.
    log: Vec<(Side, Command)>,
    /// Se o roteador entrega alguma coisa. Desligar simula perda total do meio.
    delivering: bool,
    /// Portadores que o roteador **não** entrega: um meio que morreu calado, sem aviso de queda.
    ///
    /// É o caso que a rota dupla existe para cobrir — o rádio sob interferência continua "de pé"
    /// para o sistema, e o que se manda por ele simplesmente não chega.
    silent: Vec<Carrier>,
    /// Entrega cada quadro duas vezes, para exercer a detecção de repetição.
    duplicating: bool,
    /// Descarta um quadro a cada `n`. Zero desliga.
    drop_every: u32,
    /// Quantos quadros o roteador já viu, para o padrão de descarte.
    seen: u32,
    /// Quantos quadros o roteador descartou.
    dropped: u32,
}

/// Um arranjo de uma tela só, com o tamanho dado.
pub fn layout(width: u32, height: u32) -> ScreenLayout {
    ScreenLayout::single(width, height).expect("arranjo válido")
}

/// Um arranjo de duas telas lado a lado.
pub fn dual_layout() -> ScreenLayout {
    ScreenLayout::new(vec![
        MonitorInfo {
            id: MonitorId(0),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_permille: 1000,
            primary: true,
        },
        MonitorInfo {
            id: MonitorId(1),
            x: 1920,
            y: 0,
            width: 1280,
            height: 1024,
            scale_permille: 1500,
            primary: false,
        },
    ])
    .expect("arranjo válido")
}

fn identity(name: &str, byte: u8) -> LocalIdentity {
    LocalIdentity {
        machine: MachineId([byte; 16]),
        name: MachineName::new(name).expect("nome válido"),
        capabilities: Capabilities {
            clipboard: ClipboardCapabilities::TEXT_ONLY,
            bulk_transfer: false,
            privileged_input: PrivilegedInputLevel::LockScreen,
            secure_attention: false,
        },
    }
}

impl Pair {
    /// Servidor com o par à direita, cliente com o servidor à esquerda.
    pub fn new(server_screens: ScreenLayout, client_screens: ScreenLayout) -> Self {
        Self::with_configs(
            SessionConfig::server(Edge::Right),
            SessionConfig::client(Edge::Left),
            server_screens,
            client_screens,
        )
    }

    /// Duas telas 1920×1080, com a borda que cada lado tem gravada.
    ///
    /// Para os cenários em que as duas máquinas discordam de onde fica a outra.
    pub fn with_edges(server_edge: Edge, client_edge: Edge) -> Self {
        Self::with_configs(
            SessionConfig::server(server_edge),
            SessionConfig::client(client_edge),
            layout(1920, 1080),
            layout(1920, 1080),
        )
    }

    /// Duas sessões com a configuração dada a cada uma — inclusive papéis que não combinam.
    ///
    /// Os nomes `server` e `client` são das posições na bancada, não do papel que cada uma tem.
    pub fn with_configs(
        server: SessionConfig,
        client: SessionConfig,
        server_screens: ScreenLayout,
        client_screens: ScreenLayout,
    ) -> Self {
        let mut pair = Self {
            server: Session::new(server, identity("servidor", 1)),
            client: Session::new(client, identity("cliente", 2)),
            now: Timestamp::from_millis(10_000),
            log: Vec::new(),
            delivering: true,
            silent: Vec::new(),
            duplicating: false,
            drop_every: 0,
            seen: 0,
            dropped: 0,
        };
        pair.feed(Side::Server, Input::LocalScreens(server_screens));
        pair.feed(Side::Client, Input::LocalScreens(client_screens));
        pair.feed(Side::Server, Input::AgentReady);
        pair.feed(Side::Client, Input::AgentReady);
        pair
    }

    /// Duas telas 1920×1080 iguais.
    pub fn matched() -> Self {
        Self::new(layout(1920, 1080), layout(1920, 1080))
    }

    /// O instante corrente da bancada.
    pub const fn now(&self) -> Timestamp {
        self.now
    }

    /// Avança o relógio e entrega um `Tick` para os dois lados.
    pub fn advance(&mut self, millis: u64) {
        self.now = Timestamp::from_micros(self.now.micros() + millis * 1000);
        self.feed(Side::Server, Input::Tick);
        self.feed(Side::Client, Input::Tick);
    }

    /// Avança o relógio sem entregar `Tick` a ninguém.
    pub fn advance_silently(&mut self, millis: u64) {
        self.now = Timestamp::from_micros(self.now.micros() + millis * 1000);
    }

    /// Liga ou desliga a entrega de quadros. Desligar simula perda total do meio.
    pub fn set_delivery(&mut self, delivering: bool) {
        self.delivering = delivering;
    }

    /// Cala ou devolve um portador só, sem avisar ninguém.
    pub fn set_carrier_delivery(&mut self, carrier: Carrier, delivering: bool) {
        self.silent.retain(|silent| *silent != carrier);
        if !delivering {
            self.silent.push(carrier);
        }
    }

    /// Fixa (ou solta) um portador num lado, como a interface faz.
    pub fn pin(&mut self, side: Side, carrier: Option<Carrier>) {
        let mut batch = CommandBatch::new();
        match side {
            Side::Server => self.server.pin_carrier(carrier, &mut batch),
            Side::Client => self.client.pin_carrier(carrier, &mut batch),
        }
        self.dispatch(side, &batch, 0);
    }

    /// Faz o roteador entregar cada quadro duas vezes.
    ///
    /// Um meio de datagrama duplica de verdade — por retransmissão do próprio rádio, por
    /// caminho múltiplo na rede, ou por um atacante reenviando o que gravou.
    pub fn set_duplicating(&mut self, duplicating: bool) {
        self.duplicating = duplicating;
    }

    /// Descarta um quadro a cada `n`. Zero desliga.
    pub fn set_drop_every(&mut self, n: u32) {
        self.drop_every = n;
        self.seen = 0;
        self.dropped = 0;
    }

    /// Quantos quadros o roteador descartou.
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Sobe o portador dos dois lados e conclui o handshake.
    pub fn connect(&mut self, carrier: Carrier) {
        self.feed(Side::Server, Input::CarrierUp(carrier));
        self.feed(Side::Client, Input::CarrierUp(carrier));
    }

    /// Entrega um evento a um lado e propaga o que sair.
    pub fn feed(&mut self, side: Side, input: Input) {
        let mut batch = CommandBatch::new();
        let now = self.now;
        match side {
            Side::Server => self.server.step(now, input, &mut batch),
            Side::Client => self.client.step(now, input, &mut batch),
        }
        self.dispatch(side, &batch, 0);
    }

    /// Roteia os comandos de um lote, recursivamente.
    fn dispatch(&mut self, side: Side, batch: &CommandBatch, depth: u32) {
        assert!(
            depth < 32,
            "conversa não converge: possível laço de mensagens"
        );

        let mut to_deliver = Vec::new();
        for command in batch.iter() {
            self.log.push((side, command.clone()));
            if let Command::Send { carrier, frame } = command
                && self.delivering
                && !self.silent.contains(carrier)
            {
                to_deliver.push((*carrier, frame.clone()));
            }
        }

        for (carrier, frame) in to_deliver {
            self.seen += 1;
            if self.drop_every > 0 && self.seen.is_multiple_of(self.drop_every) {
                self.dropped += 1;
                continue;
            }
            let copies = if self.duplicating { 2 } else { 1 };
            for _ in 0..copies {
                let mut batch = CommandBatch::new();
                let now = self.now;
                let input = Input::Received {
                    carrier,
                    frame: frame.clone(),
                };
                match side.other() {
                    Side::Server => self.server.step(now, input, &mut batch),
                    Side::Client => self.client.step(now, input, &mut batch),
                }
                self.dispatch(side.other(), &batch, depth + 1);
            }
        }
    }

    /// Quantos quadros um lado mandou por este portador, desde a última limpeza.
    pub fn sent_on(&self, side: Side, carrier: Carrier) -> usize {
        self.count(
            side,
            |command| matches!(command, Command::Send { carrier: c, .. } if *c == carrier),
        )
    }

    /// Tudo que foi pedido desde a última chamada, e esvazia o registro.
    pub fn take_log(&mut self) -> Vec<(Side, Command)> {
        core::mem::take(&mut self.log)
    }

    /// Esvazia o registro sem devolver nada.
    pub fn clear_log(&mut self) {
        self.log.clear();
    }

    /// Os comandos de um lado, desde a última limpeza.
    pub fn commands(&self, side: Side) -> Vec<Command> {
        self.log
            .iter()
            .filter(|(s, _)| *s == side)
            .map(|(_, c)| c.clone())
            .collect()
    }

    /// Se um lado pediu algum comando que case com o teste dado.
    pub fn any(&self, side: Side, predicate: impl Fn(&Command) -> bool) -> bool {
        self.log.iter().any(|(s, c)| *s == side && predicate(c))
    }

    /// Quantos comandos de um lado casam com o teste dado.
    pub fn count(&self, side: Side, predicate: impl Fn(&Command) -> bool) -> usize {
        self.log
            .iter()
            .filter(|(s, c)| *s == side && predicate(c))
            .count()
    }

    /// A posição do primeiro `ReleaseAll` de um lado no registro, se houver.
    pub fn index_of_release(&self, side: Side) -> Option<usize> {
        self.log
            .iter()
            .filter(|(s, _)| *s == side)
            .position(|(_, c)| matches!(c, Command::ReleaseAll))
    }

    /// A posição do primeiro comando de um lado que case, se houver.
    pub fn index_of(&self, side: Side, predicate: impl Fn(&Command) -> bool) -> Option<usize> {
        self.log
            .iter()
            .filter(|(s, _)| *s == side)
            .position(|(_, c)| predicate(c))
    }

    /// Todos os avisos emitidos por um lado.
    pub fn notices(&self, side: Side) -> Vec<Notice> {
        self.log
            .iter()
            .filter_map(|(s, c)| match c {
                Command::Notify(notice) if *s == side => Some(notice.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Testes prontos, para as asserções ficarem legíveis nos cenários.
pub mod is {
    use super::Command;
    use ir_proto::message::{Control, Message};

    pub fn release_all(command: &Command) -> bool {
        matches!(command, Command::ReleaseAll)
    }

    pub fn injection(command: &Command) -> bool {
        matches!(command, Command::Inject(_))
    }

    pub fn suppress(command: &Command) -> bool {
        matches!(command, Command::SuppressLocalInput(true))
    }

    pub fn unsuppress(command: &Command) -> bool {
        matches!(command, Command::SuppressLocalInput(false))
    }

    pub fn warp(command: &Command) -> bool {
        matches!(command, Command::WarpPointer(_))
    }

    pub fn enter_screen(command: &Command) -> bool {
        matches!(
            command,
            Command::Send { frame, .. }
                if matches!(frame.message, Message::Control(Control::EnterScreen { .. }))
        )
    }

    pub fn snapshot(command: &Command) -> bool {
        matches!(
            command,
            Command::Send { frame, .. }
                if matches!(frame.message, Message::Control(Control::StateSnapshot { .. }))
        )
    }

    pub fn leave_screen(command: &Command) -> bool {
        matches!(
            command,
            Command::Send { frame, .. }
                if matches!(frame.message, Message::Control(Control::LeaveScreen { .. }))
        )
    }
}
