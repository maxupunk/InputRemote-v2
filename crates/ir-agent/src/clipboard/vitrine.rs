//! O ícone da bandeja e o do dock, no Linux: o que está acontecendo, à vista sem abrir nada.
//!
//! No Windows esse papel é da interface, que mora na bandeja. No Linux a janela costuma estar
//! fechada, e quem vive a sessão inteira é este ajudante — que já recebe tudo o que importa: as
//! cópias, o estado da conexão e as pastas.
//!
//! - **A bandeja** ([`ir_recado::linux::bandeja`]): o mesmo ícone do Windows, com o menu que diz,
//!   numa linha, o que está acontecendo.
//! - **O dock** ([`ir_recado::linux::doca`]): a barra de andamento da cópia sobre o ícone do
//!   InputRemote — no GNOME, a notificação não desenha barra; o Dash to Dock desenha.
//!
//! Fora do Linux, nada: quem mostra é a interface.

use ir_ipc::Estado;
use ir_ipc::pastas::{ResumoDePasta, SituacaoDaPasta};
use ir_ipc::transferencia::Transferencia;
use ir_recado::Tom;

/// O que o ícone conta, e onde.
#[derive(Debug, Default)]
pub(crate) struct Vitrine {
    #[cfg(target_os = "linux")]
    bandeja: Option<ir_recado::linux::bandeja::Bandeja>,
    #[cfg(target_os = "linux")]
    doca: Option<ir_recado::linux::doca::Doca>,
    /// Sem o outro computador, ou pausado.
    parado: bool,
    /// Uma pasta compartilhada sincronizando agora.
    sincronizando: bool,
    /// Como anda, ou como terminou, a última cópia.
    copia: Option<Tom>,
    /// A frase da cópia, enquanto ela anda ou quando ela não atravessou.
    frase_da_copia: Option<String>,
    /// A frase do estado da conexão.
    resumo: String,
}

impl Vitrine {
    /// Põe os ícones. Sem barramento da sessão, fica sem eles, e o resto segue.
    pub(crate) fn abrir() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            bandeja: ir_recado::linux::bandeja::Bandeja::abrir(abrir_a_janela),
            #[cfg(target_os = "linux")]
            doca: ir_recado::linux::doca::Doca::abrir(),
            parado: true,
            ..Self::default()
        }
    }

    /// O estado da conexão mudou.
    pub(crate) fn estado(&mut self, estado: &Estado) {
        self.parado = !estado.enlace.conectado() || estado.pausa.is_some();
        self.resumo = estado.resumo();
        self.retratar();
    }

    /// As pastas compartilhadas mudaram.
    pub(crate) fn pastas(&mut self, lista: &[ResumoDePasta]) {
        self.sincronizando = lista
            .iter()
            .any(|pasta| pasta.situacao == SituacaoDaPasta::Sincronizando);
        self.retratar();
    }

    /// Uma cópia andou, chegou, ou não atravessou.
    pub(crate) fn copia(&mut self, copia: &Transferencia) {
        let tom = Tom::da_copia(copia);
        self.copia = Some(tom);
        self.frase_da_copia =
            (tom != Tom::Feito).then(|| format!("{}: {}", copia.titulo(), copia.detalhe()));
        #[cfg(target_os = "linux")]
        if let Some(doca) = &mut self.doca {
            doca.andamento(copia.em_curso().then(|| copia.progresso()));
        }
        self.retratar();
    }

    /// Leva o que acontece ao ícone.
    #[cfg_attr(not(target_os = "linux"), allow(clippy::unused_self))]
    fn retratar(&self) {
        #[cfg(target_os = "linux")]
        if let Some(bandeja) = &self.bandeja {
            let retrato = ir_recado::bandeja::aparencia::Retrato {
                parado: self.parado,
                atravessando: self.sincronizando,
                copia: self.copia,
                janela_visivel: false,
            };
            bandeja.retratar(
                retrato,
                self.frase_da_copia.as_ref().unwrap_or(&self.resumo),
            );
        }
    }
}

/// Abre a janela do InputRemote: o clique no ícone e o item do menu.
#[cfg(target_os = "linux")]
fn abrir_a_janela() {
    if let Err(erro) = std::process::Command::new("inputremote-ui").spawn() {
        tracing::debug!(%erro, "não consegui abrir a janela");
    }
}
