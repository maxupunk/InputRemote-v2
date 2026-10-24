//! O painel no Android: quem decide é a [`ir_vigilancia`]; o serviço só dirige a sessão com o que
//! ela devolve.

use ir_session::Input;
use ir_vigilancia::Evento;

use super::Daemon;

impl Daemon {
    /// Um aviso do monitor, ou a amostra deste segundo.
    pub(crate) fn on_painel(&mut self, evento: Evento) {
        let entradas = self.painel.evento(evento, &mut self.config, &self.gravador);
        self.dirigir(entradas);
    }

    /// A batida de cada segundo do painel.
    pub(crate) fn bater_painel(&mut self) {
        let entradas = self.painel.batida(&self.session, &self.config);
        self.dirigir(entradas);
    }

    fn dirigir(&mut self, entradas: Vec<Input>) {
        for entrada in entradas {
            self.drive(entrada);
        }
    }
}
