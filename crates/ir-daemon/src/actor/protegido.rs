//! O desktop protegido — tela de bloqueio, login, UAC — visto do serviço.
//!
//! Digitar ali é opcional e desligado por padrão, por par ([04, §6](../../../docs/04-seguranca.md)).
//! Com a permissão desligada, o que o par digita é descartado **e o par fica sabendo**, para quem
//! digita não achar que o produto travou. Com ela ligada, o par também pode pedir Ctrl+Alt+Del.
//!
//! No Windows quem recusa é o agente, que sabe em que desktop está injetando. No Linux o serviço
//! injeta direto por `uinput`, e pergunta ao `logind` se a sessão da tela está bloqueada.

use ir_ipc::{Aviso, Falha};
#[cfg(any(not(windows), test))]
use ir_proto::input::HidUsage;
use ir_session::{Injection, Input};
use tracing::{info, warn};

use super::Daemon;

/// As teclas do Ctrl+Alt+Del, para onde ele é só um acorde (o Linux).
#[cfg(not(windows))]
const CTRL_ALT_DEL: [HidUsage; 3] = [HidUsage(0xE0), HidUsage(0xE2), HidUsage(0x4C)];

impl Daemon {
    /// Se o par pode digitar no desktop protegido daqui.
    pub(crate) fn protegido_permitido(&self) -> bool {
        self.config
            .peers
            .first()
            .is_some_and(|par| par.tela_de_bloqueio)
    }

    /// Passou a recusar, ou voltou a aceitar, digitação no desktop protegido: o par fica sabendo.
    pub(crate) fn recusando_protegido(&mut self, recusando: bool) {
        if self.recusa_protegido == recusando {
            return;
        }
        self.recusa_protegido = recusando;
        if recusando {
            info!("digitação do par na tela de bloqueio recusada: a permissão está desligada");
        }
        self.drive(Input::LocalProtectedDesktop(recusando));
    }

    /// O par contou que recusa, ou voltou a aceitar, digitação daqui no desktop protegido dele.
    pub(crate) fn on_par_recusa_protegido(&mut self, recusa: bool) {
        self.par_recusa_protegido = recusa;
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
    }

    /// O par pediu Ctrl+Alt+Del aqui.
    pub(crate) fn gerar_ctrl_alt_del(&mut self) {
        if !self.protegido_permitido() {
            warn!("o par pediu Ctrl+Alt+Del, e a permissão da tela de bloqueio está desligada");
            return;
        }
        info!("o par pediu Ctrl+Alt+Del");
        #[cfg(windows)]
        {
            let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                return;
            };
            runtime.spawn_blocking(|| {
                use ir_sessao::atencao;
                let politica = atencao::politica();
                if !politica.permite_servicos() {
                    // A permissão foi dada por um administrador daqui, e é ela que liga a política;
                    // se alguém a desligou por fora, o pedido não gera nada, e isso vai ao registro.
                    warn!(
                        ?politica,
                        "a política do Windows não deixa o serviço gerar Ctrl+Alt+Del"
                    );
                    return;
                }
                atencao::enviar_sas();
            });
        }
        #[cfg(not(windows))]
        {
            // Onde Ctrl+Alt+Del é só um acorde, ele é digitado como qualquer outro.
            for pressionada in [true, false] {
                let ordem: Vec<HidUsage> = if pressionada {
                    CTRL_ALT_DEL.to_vec()
                } else {
                    CTRL_ALT_DEL.iter().rev().copied().collect()
                };
                for usage in ordem {
                    self.injetar_direto(Injection::Key {
                        usage,
                        pressed: pressionada,
                    });
                }
            }
        }
    }

    /// O pedido de Ctrl+Alt+Del daqui não saiu: o par é antigo, ou não há sessão.
    pub(crate) fn ctrl_alt_del_nao_saiu(&self) {
        let falha = if self.session.phase().is_established() {
            Falha::ParDesatualizado
        } else {
            Falha::SemConexao
        };
        let _ = self.avisos.send(Aviso::Falhou(falha));
    }

    /// Se esta injeção deve ser descartada por cair no desktop protegido sem permissão (Linux).
    ///
    /// Soltar passa sempre: uma tecla que desceu antes do bloqueio tem de poder subir.
    pub(crate) fn barrar_no_protegido(&mut self, injecao: Injection) -> bool {
        if !self.tela_protegida || self.protegido_permitido() {
            return false;
        }
        let soltando = matches!(
            injecao,
            Injection::Key { pressed: false, .. } | Injection::Button { pressed: false, .. }
        );
        if soltando {
            return false;
        }
        self.recusando_protegido(true);
        true
    }

    /// Liga, ou devolve, a política do Windows que deixa o serviço gerar Ctrl+Alt+Del.
    ///
    /// É o mesmo consentimento da tela de bloqueio, dado pelo mesmo administrador, e por isso anda
    /// junto com ele ([04, §6](../../../docs/04-seguranca.md)). O `reg.exe` roda aqui, no laço:
    /// é uma ação rara, pedida à mão e com elevação, e a configuração precisa sair dela já com o
    /// valor anterior guardado. Devolve a configuração a gravar.
    #[cfg(windows)]
    #[allow(clippy::unused_self)] // mora no ator com o resto da permissão, que é dele
    pub(crate) fn politica_de_atencao(
        &self,
        mut nova: crate::config::Config,
        permitir: bool,
    ) -> crate::config::Config {
        use ir_sessao::atencao::{PoliticaDeAtencao, gravar_politica, politica};
        if !ir_sessao::como_servico() {
            return nova; // em primeiro plano, como usuário, a política da máquina não é nossa
        }
        if permitir {
            let atual = politica();
            if !atual.permite_servicos() {
                match gravar_politica(atual.com_servicos()) {
                    Ok(()) => {
                        info!(?atual, "política de Ctrl+Alt+Del ligada para o serviço");
                        nova.politica_de_atencao_anterior = Some(atual.numero());
                    }
                    Err(erro) => warn!(%erro, "não foi possível ligar a política de Ctrl+Alt+Del"),
                }
            }
        } else if let Some(anterior) = nova.politica_de_atencao_anterior.take()
            && let Err(erro) = gravar_politica(PoliticaDeAtencao::do_numero(anterior))
        {
            warn!(%erro, "não foi possível devolver a política de Ctrl+Alt+Del");
        }
        nova
    }

    /// O `logind` disse se a tela desta máquina está bloqueada, ou no login (Linux).
    pub(crate) fn on_tela_protegida(&mut self, protegida: bool) {
        self.tela_protegida = protegida;
        if !protegida {
            self.recusando_protegido(false);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::actor::bancada::Bancada;
    use crate::config::PinnedPeer;

    fn com_par(bancada: &mut Bancada, permitido: bool) {
        bancada.daemon.config.peers = vec![PinnedPeer {
            pubkey: "00".repeat(32),
            addr: None,
            radio: None,
            nome: None,
            tela_de_bloqueio: permitido,
        }];
    }

    fn tecla(pressed: bool) -> Injection {
        Injection::Key {
            usage: HidUsage(0x04),
            pressed,
        }
    }

    #[test]
    fn com_a_tela_bloqueada_e_sem_permissao_a_tecla_nao_entra_mas_solta() {
        let mut bancada = Bancada::nova();
        com_par(&mut bancada, false);
        bancada.daemon.on_tela_protegida(true);

        assert!(bancada.daemon.barrar_no_protegido(tecla(true)));
        assert!(
            !bancada.daemon.barrar_no_protegido(tecla(false)),
            "soltar passa sempre"
        );
        assert!(bancada.daemon.recusa_protegido);

        bancada.daemon.on_tela_protegida(false);
        assert!(
            !bancada.daemon.recusa_protegido,
            "desbloqueou: volta a aceitar"
        );
        assert!(!bancada.daemon.barrar_no_protegido(tecla(true)));
    }

    #[test]
    fn com_permissao_a_tela_bloqueada_recebe_digitacao() {
        let mut bancada = Bancada::nova();
        com_par(&mut bancada, true);
        bancada.daemon.on_tela_protegida(true);
        assert!(!bancada.daemon.barrar_no_protegido(tecla(true)));
    }

    #[test]
    fn a_recusa_do_par_aparece_no_estado() {
        let mut bancada = Bancada::nova();
        bancada.daemon.on_par_recusa_protegido(true);
        assert!(bancada.daemon.par_recusa_protegido);
        assert!(
            !bancada.daemon.estado().par_recusa_tela_de_bloqueio,
            "sem sessão, o que o par disse não vale mais"
        );
        bancada.daemon.on_par_recusa_protegido(false);
        assert!(!bancada.daemon.par_recusa_protegido);
    }
}
