//! O recado do painel no Android entre os computadores, e a latência de cada rota.
//!
//! O recado ([`Control::Panel`]) é opaco aqui: o pedido de amostras, a amostra e os processos são do
//! vocabulário do painel (`ir_monitor::entre_pcs`). A sessão só o leva, e só a um par da versão 11
//! ([12](../../../../docs/12-painel-android.md)).
//!
//! A latência por rota vem do `Pong` que já existe. Na rota dupla o `Pong` chega pelos dois
//! portadores; só o primeiro passa da detecção de repetição, então a medida de cada portador é
//! tirada **antes** dela ([`Session::measure_route`]). O par responde ao primeiro `Ping` que lhe
//! chega, então a medida do portador mais lento é a ida pelo mais rápido somada à volta por ele: é
//! uma estimativa honesta de "quanto esta rota atrasa", e exata quando a rota tem um portador só.

use ir_proto::carrier::Carrier;
use ir_proto::frame::Frame;
use ir_proto::message::{Control, Message};
use ir_proto::{limits, version};

use crate::event::{Command, CommandBatch, Notice};
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    /// Se o par entende o recado do painel: há sessão, e ele é da versão 11.
    ///
    /// Exposta para o serviço mostrar só esta máquina no painel, em vez de esperar ao vento pela
    /// amostra de um par antigo.
    #[must_use]
    pub fn peer_has_panel(&self) -> bool {
        self.peer_understands(version::supports_panel)
    }

    /// Leva um recado do painel ao par. Um maior que o teto não sai: quem o montou errou, e ele não
    /// caberia no quadro do rádio.
    pub(super) fn send_panel(&mut self, now: Timestamp, recado: Vec<u8>, out: &mut CommandBatch) {
        if self.peer_has_panel() && recado.len() <= limits::MAX_PANEL_MESSAGE {
            self.send(now, Message::Control(Control::Panel(recado)), out);
        }
    }

    /// Se o quadro é um `Pong`, anota a volta pelo portador que o trouxe.
    ///
    /// Chamada antes da detecção de repetição, que descartaria a cópia do portador mais lento.
    pub(super) fn measure_route(
        now: Timestamp,
        carrier: Carrier,
        frame: &Frame,
        out: &mut CommandBatch,
    ) {
        let Message::Control(Control::Pong { stamp_micros }) = frame.message else {
            return;
        };
        let sent = Timestamp::from_micros(stamp_micros);
        if sent > now {
            return; // carimbo do futuro: só pode ser de outra sessão
        }
        out.push(Command::Notify(Notice::RouteLatency {
            carrier,
            rtt: now.since(sent),
        }));
    }
}
