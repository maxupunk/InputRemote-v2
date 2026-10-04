//! A notificação do sistema, no Linux.
//!
//! No Windows a interface fica na bandeja e mostra a notificação nativa. No GNOME não há bandeja, e
//! a janela costuma estar fechada — quem copia está no Nautilus. O lugar certo do recado ali é a
//! notificação do sistema, e quem sempre está de pé na sessão é este ajudante.
//!
//! O que dizer é o mesmo dos dois sistemas, e mora em [`ir_recado`]. No Linux, a cópia em curso é
//! contada só pelo ícone da bandeja e pelo do dock ([`super::vitrine`]), que não tiram a pessoa do
//! que ela está fazendo; a notificação aparece uma vez, no fim. Um recado de andamento que se
//! substituía a cada passo piscava na tela e chamava a atenção para algo que não pede nada.

use ir_ipc::Aviso;
use ir_ipc::transferencia::Transferencia;
use ir_recado::Recado;

use super::vitrine::Vitrine;

/// Conta ao usuário as cópias e o que mais acontece sem a janela aberta.
#[derive(Debug)]
pub(crate) struct Notificador {
    vitrine: Vitrine,
}

impl Notificador {
    /// Pronto para contar; no Linux, com o ícone na bandeja e o andamento no dock.
    pub(crate) fn abrir() -> Self {
        Self {
            vitrine: Vitrine::abrir(),
        }
    }

    /// O que um aviso do serviço muda no que a pessoa vê.
    pub(crate) fn aviso(&mut self, aviso: &Aviso) {
        match aviso {
            Aviso::Transferencia(copia) => self.contar(copia),
            // O ícone fica cinza sem o outro computador, e gira enquanto uma pasta sincroniza.
            Aviso::EstadoMudou(estado) => self.vitrine.estado(estado),
            Aviso::PastasMudaram(lista) => self.vitrine.pastas(lista),
            // O outro computador mudou copiar e colar, e este acompanhou: quem está aqui precisa
            // saber por que o Ctrl+C parou (ou voltou) sem ter mexido em nada.
            Aviso::CopiarEColarAjustado { ligado, par } => {
                Self::mostrar(&Recado::copiar_e_colar_ajustado(*ligado, par));
            }
            _ => {}
        }
    }

    /// Conta esta cópia: o ícone acompanha cada passo; a notificação, só o fim.
    fn contar(&mut self, copia: &Transferencia) {
        self.vitrine.copia(copia);
        if copia.terminou() {
            Self::mostrar(&Recado::da_copia(copia));
        }
    }

    #[cfg(target_os = "linux")]
    fn mostrar(recado: &Recado) {
        ir_recado::linux::avisar(recado);
    }

    /// Fora do Linux quem conta é a interface, com a notificação nativa.
    #[cfg(not(target_os = "linux"))]
    const fn mostrar(_recado: &Recado) {}
}
