//! O cursor conduzido pelo serviço, no Linux.
//!
//! O Wayland não conta a ninguém onde o cursor está. Enquanto o serviço só ouvia os deslocamentos
//! e o compositor movia o cursor com a aceleração dele, o modelo da sessão e o cursor real andavam
//! em velocidades diferentes, e a travessia vinha no meio da tela — nenhum ajuste de escala fecha
//! isso, e a volta do par, que devia realinhar os dois, não movia o cursor (log 50).
//!
//! Então, com o teclado aqui, o serviço **conduz** o cursor: a captura toma o mouse e o touchpad
//! (`Capturer::conduzir_o_cursor`), a sessão move o modelo, e este módulo põe o cursor do compositor
//! exatamente no modelo, pelo ponteiro virtual absoluto — o mesmo pelo qual o outro computador
//! controla este. Botões e roda, também tomados, são repostos pelo mesmo caminho. O teclado não é
//! tomado com o controle aqui, e segue direto para o compositor.
//!
//! Só onde há captura **e** injetor: sem os dois, nada é tomado, e a máquina fica como antes.

use ir_input::{CaptureEvent, InjectEvent};
use ir_session::{Input, Phase, Role};
use tracing::{debug, info};

use super::Daemon;

/// O estado da condução do cursor.
#[derive(Debug, Default)]
pub(crate) struct Conducao {
    /// Se o serviço conduz o cursor desta máquina.
    pub(crate) ligada: bool,
    /// Onde o cursor foi posto por último, nas coordenadas da sessão.
    ultimo: Option<(i32, i32)>,
}

impl Daemon {
    /// Liga a condução se esta máquina tem o teclado e pode capturar e injetar; desliga no resto.
    ///
    /// No Windows o serviço nunca tem captura nem injetor próprios — são do agente, que conta a
    /// posição real do cursor —, e a condução não liga.
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn ajustar_conducao(&mut self) {
        let pode = self.session.role() == Role::Server
            && self.capturer.is_some()
            && self.injector.is_some();
        if pode == self.cursor.ligada {
            return;
        }
        self.cursor.ligada = pode;
        if let Some(capturador) = self.capturer.as_ref() {
            capturador.conduzir_o_cursor(pode);
        }
        self.cursor.ultimo = None;
        if pode {
            info!("este computador tem o teclado: o serviço passa a conduzir o cursor daqui");
            // O cursor real vai uma vez para o meio da tela, e dali em diante é o modelo.
            self.seed_pointer = false;
            let (largura, altura) = self.screen;
            self.session.sync_pointer(
                i32::try_from(largura / 2).unwrap_or(0),
                i32::try_from(altura / 2).unwrap_or(0),
            );
            self.mover_cursor();
        } else {
            info!("o serviço deixou de conduzir o cursor daqui");
        }
    }

    /// A cada segundo: sem esta renovação, a captura devolve o mouse ao sistema em 3 s.
    pub(crate) fn renovar_conducao(&self) {
        if self.cursor.ligada
            && let Some(capturador) = self.capturer.as_ref()
        {
            capturador.conduzir_o_cursor(true);
        }
    }

    /// Se o controle está aqui e o serviço conduz o cursor.
    fn conduzindo_aqui(&self) -> bool {
        self.cursor.ligada && self.session.phase() != Phase::Engaged
    }

    /// Antes de um evento capturado: uma sessão recriada (troca de papel, reconexão) nasce com o
    /// ponteiro em outro lugar; ela passa a partir de onde o cursor real está.
    pub(crate) fn modelo_no_cursor(&mut self) {
        if self.conduzindo_aqui()
            && let Some((x, y)) = self.cursor.ultimo
            && self.session.pointer_xy() != (x, y)
        {
            self.session.sync_pointer(x, y);
        }
    }

    /// Depois de um evento capturado com o controle aqui: o cursor acompanha o modelo, e o botão e
    /// a roda, que a captura tomou, chegam ao compositor.
    pub(crate) fn repor_localmente(&mut self, evento: CaptureEvent) {
        if !self.conduzindo_aqui() {
            return;
        }
        match evento {
            CaptureEvent::PointerMotion { .. } => self.mover_cursor(),
            CaptureEvent::Button { button, pressed } => {
                self.injetar_aqui(InjectEvent::Button { button, pressed });
            }
            CaptureEvent::Wheel(delta) => self.injetar_aqui(InjectEvent::Wheel(delta)),
            _ => {}
        }
    }

    /// A sessão pôs o ponteiro em outro lugar (a volta do par): o cursor real vai junto. Devolve se
    /// o serviço conduz o cursor — senão quem move é outro caminho.
    pub(crate) fn levar_cursor(&mut self) -> bool {
        if !self.cursor.ligada {
            return false;
        }
        self.mover_cursor();
        true
    }

    /// Põe o cursor do compositor onde o modelo da sessão está, se ele mudou.
    fn mover_cursor(&mut self) {
        let onde = self.session.pointer_xy();
        if self.cursor.ultimo == Some(onde) {
            return;
        }
        if let Some(posicao) = self.session.pointer_position() {
            self.injetar_aqui(InjectEvent::Pointer(posicao));
            self.cursor.ultimo = Some(onde);
        }
    }

    fn injetar_aqui(&mut self, evento: InjectEvent) {
        if let Some(injetor) = self.injector.as_mut()
            && let Err(erro) = injetor.inject(evento)
        {
            debug!(%erro, "o cursor conduzido não foi aceito pelo sistema");
        }
    }

    /// Um evento capturado localmente (papel de servidor), com a condução do cursor em volta.
    pub(crate) fn capturado(&mut self, evento: CaptureEvent, entrada: Input) {
        self.modelo_no_cursor();
        self.drive(entrada);
        self.repor_localmente(evento);
    }
}

#[cfg(test)]
mod tests;
