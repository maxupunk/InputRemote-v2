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
use crate::config::{Config, PinnedPeer};

/// As teclas do Ctrl+Alt+Del, para onde ele é só um acorde (o Linux).
#[cfg(not(windows))]
const CTRL_ALT_DEL: [HidUsage; 3] = [HidUsage(0xE0), HidUsage(0xE2), HidUsage(0x4C)];

/// Se o par gravado nesta configuração pode digitar no desktop protegido daqui.
///
/// Sem par, ninguém pode — e é isso que faz esquecer o par devolver a política do Windows. É também
/// o que decide a política: ligada exatamente quando isto é verdade.
fn permitido_em(config: &Config) -> bool {
    config
        .peers
        .first()
        .is_some_and(PinnedPeer::permite_tela_de_bloqueio)
}

/// Quem aplica a política de Ctrl+Alt+Del do Windows, só quando este processo é o serviço.
#[cfg(windows)]
pub(super) fn aplicador_de_atencao(
    config: &Config,
    de_fundo: &tokio::sync::mpsc::UnboundedSender<super::DeFundo>,
) -> Option<ir_sessao::atencao::Aplicador> {
    if !ir_sessao::como_servico() {
        return None; // em primeiro plano, como usuário, a política da máquina não é nossa
    }
    let de_fundo = de_fundo.clone();
    Some(ir_sessao::atencao::Aplicador::novo(
        config.politica_de_atencao_anterior,
        move |anterior| {
            let _ = de_fundo.send(super::DeFundo::PoliticaDeAtencao(anterior));
        },
    ))
}

impl Daemon {
    /// Se o par pode digitar no desktop protegido daqui.
    pub(crate) fn protegido_permitido(&self) -> bool {
        permitido_em(&self.config)
    }

    /// A permissão do desktop protegido pode ter mudado — ligada, desligada, par novo, par
    /// esquecido —, e a configuração nova já está gravada: a mudança passa a valer em todo lugar.
    ///
    /// Um caminho só. Eram quatro sequências, e divergiram: esquecer o par não contava ao agente, não
    /// reavaliava a tela já protegida e não devolvia a política do Windows; parear não reavaliava a
    /// tela. Quem chama avisa a janela, junto com o que mais mudou.
    pub(crate) fn permissao_do_protegido_mudou(&mut self) {
        self.alinhar_politica_de_atencao();
        self.contar_ao_agente_a_permissao();
        // Com a tela já protegida, a recusa muda na hora: o par ganha ou perde a parede.
        let protegida = self.tela_protegida;
        self.on_tela_protegida(protegida);
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
        self.avisar_estado();
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
            self.em_fundo(|_| {
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

    /// Leva a política do Windows que deixa o serviço gerar Ctrl+Alt+Del ao que a permissão pede:
    /// ligada com ela, devolvida ao que era sem ela — ou sem par.
    ///
    /// É o mesmo consentimento da tela de bloqueio, dado pelo mesmo administrador, e por isso anda
    /// junto com ele ([04, §6](../../../docs/04-seguranca.md)); na subida, sozinha, sem ninguém ter de
    /// passar pelas Preferências (log 53). O `reg.exe` roda fora do laço, em ordem
    /// ([`ir_sessao::atencao::Aplicador`]), e o valor de antes volta por [`Self::on_politica_de_atencao`].
    #[cfg_attr(not(windows), allow(clippy::unused_self, clippy::missing_const_for_fn))]
    pub(crate) fn alinhar_politica_de_atencao(&self) {
        #[cfg(windows)]
        if let Some(aplicador) = &self.atencao {
            aplicador.pedir(self.protegido_permitido());
        }
    }

    /// O valor a devolver mudou: gravado, para ser devolvido mesmo depois de reiniciar.
    pub(crate) fn on_politica_de_atencao(&mut self, anterior: Option<u32>) {
        if self.config.politica_de_atencao_anterior != anterior {
            self.gravar_ja(|config| config.politica_de_atencao_anterior = anterior);
        }
    }

    /// O `logind` disse se a tela desta máquina está bloqueada, ou no login (Linux).
    ///
    /// Sem a permissão, o par fica sabendo **na hora**, e não só depois da primeira tecla barrada:
    /// antes o cursor dele atravessava e ficava preso aqui, sem efeito nenhum (log 52). Sabendo, a
    /// borda dele vira parede, e a tela dele diz por quê.
    pub(crate) fn on_tela_protegida(&mut self, protegida: bool) {
        self.tela_protegida = protegida;
        self.recusando_protegido(protegida && !self.protegido_permitido());
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::actor::bancada::Bancada;

    fn com_par(bancada: &mut Bancada, permitido: bool) {
        bancada.daemon.config.peers = vec![PinnedPeer {
            pubkey: "00".repeat(32),
            addr: None,
            radio: None,
            nome: None,
            recusa_tela_de_bloqueio: !permitido,
        }];
    }

    fn tecla(pressed: bool) -> Injection {
        Injection::Key {
            usage: HidUsage(0x04),
            pressed,
        }
    }

    #[test]
    fn a_recusa_e_anunciada_assim_que_a_tela_bloqueia_e_nao_so_na_primeira_tecla() {
        // Log 52: sabendo antes, a borda do par vira parede, e o cursor dele não fica preso aqui.
        let mut sem = Bancada::nova();
        com_par(&mut sem, false);
        sem.daemon.on_tela_protegida(true);
        assert!(sem.daemon.recusa_protegido);

        let mut com = Bancada::nova();
        com_par(&mut com, true);
        com.daemon.on_tela_protegida(true);
        assert!(
            !com.daemon.recusa_protegido,
            "com a permissão, não há o que recusar"
        );
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
    fn a_politica_do_windows_fica_ligada_so_com_par_e_permissao() {
        // A decisão que leva a política: sem par — esquecido —, ela é devolvida, e não fica ligada
        // para quem não pode mais digitar aqui.
        let mut bancada = Bancada::nova();
        assert!(!permitido_em(&bancada.daemon.config), "sem par");
        com_par(&mut bancada, true);
        assert!(permitido_em(&bancada.daemon.config));
        com_par(&mut bancada, false);
        assert!(!permitido_em(&bancada.daemon.config), "o par recusado");
        com_par(&mut bancada, true);
        let _ = bancada.daemon.esquecer_par();
        assert!(!permitido_em(&bancada.daemon.config), "o par esquecido");
    }

    #[test]
    fn esquecer_o_par_recusa_na_hora_a_tela_ja_protegida_e_conta_ao_agente() {
        // Esquecer o par não reavaliava nada: com a tela bloqueada, o par esquecido seguia sem parede,
        // e o agente seguia deixando digitar no desktop protegido.
        let mut bancada = Bancada::nova();
        com_par(&mut bancada, true);
        bancada.daemon.on_fato(ir_ipc::FatoDoAgente::Pronto {
            desktops: vec!["Default".to_owned(), "Winlogon".to_owned()],
            tela_de_bloqueio: true,
        });
        bancada.daemon.on_tela_protegida(true);
        assert!(!bancada.daemon.recusa_protegido);
        while bancada.agente.try_recv().is_ok() {}

        assert_eq!(bancada.daemon.esquecer_par(), ir_ipc::Resposta::Feito);

        assert!(bancada.daemon.recusa_protegido, "a parede sobe na hora");
        let mut contou = false;
        while let Ok(comando) = bancada.agente.try_recv() {
            contou |= comando == ir_ipc::ComandoDoAgente::PermitirDesktopProtegido(false);
        }
        assert!(contou, "o agente precisa saber que não pode mais");
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
