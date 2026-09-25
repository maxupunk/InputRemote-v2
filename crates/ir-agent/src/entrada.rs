//! O que o agente faz na máquina local a mando do serviço: injetar, suprimir, prender e soltar.

use std::sync::{Arc, Mutex};

use ir_geometry::Desktop;
use ir_input::{Capturer, InjectEvent, Injector};
use ir_ipc::{ComandoDoAgente, FatoDoAgente};
use ir_proto::input::PointerPosition;
use ir_proto::screens::ScreenLayout;
use tracing::warn;

use crate::{Escrita, enviar};

/// O que o agente tem para mexer na máquina local.
pub(crate) struct Entrada {
    pub(crate) injetor: Option<Box<dyn Injector>>,
    pub(crate) capturador: Option<Box<dyn Capturer>>,
    /// Os monitores desta sessão, para prender o ponteiro no pixel certo.
    pub(crate) telas: Option<Desktop>,
}

impl Entrada {
    /// O arranjo de telas desta sessão chegou, ou mudou: o injetor e o ponteiro preso passam a usá-lo.
    pub(crate) fn usar_telas(&mut self, arranjo: &ScreenLayout) {
        if let Some(injetor) = self.injetor.as_mut() {
            injetor.usar_telas(arranjo);
        }
        self.telas = Desktop::from_layout(arranjo);
    }

    /// Executa um comando do serviço.
    pub(crate) fn executar(&mut self, comando: ComandoDoAgente, escrita: &Arc<Mutex<Escrita>>) {
        match comando {
            ComandoDoAgente::Injetar(evento) => self.injetar(evento, escrita),
            ComandoDoAgente::SoltarTudo => self.soltar_tudo(),
            ComandoDoAgente::SuprimirEntradaLocal(ligado) => {
                if let Some(capturador) = self.capturador.as_ref() {
                    capturador.set_suppress(ligado);
                }
            }
            ComandoDoAgente::PrenderPonteiro(posicao) => self.prender(posicao),
            ComandoDoAgente::BloquearTela => {
                if !ir_input::bloquear_a_tela() {
                    warn!("o sistema não bloqueou a tela a pedido do par");
                }
            }
            ComandoDoAgente::PermitirDesktopProtegido(permitir) => {
                if let Some(injetor) = self.injetor.as_mut() {
                    injetor.permitir_desktop_protegido(permitir);
                }
            }
            // A Sequência de Atenção Segura é N2: depende de `SendSAS` e da política do sistema
            // ([05, §4.3](../../../docs/05-windows.md)), e entra com a tela de bloqueio.
            ComandoDoAgente::SequenciaDeAtencao => {
                warn!("Ctrl+Alt+Del pedido, mas ainda não implementado");
            }
            // `Encerrar` é do laço principal, que sai antes de chegar aqui; o curinga cobre as
            // variantes futuras do enum não exaustivo.
            _ => {}
        }
    }

    /// Injeta o que o comando pedir, e conta ao serviço se o sistema recusar.
    fn injetar(&mut self, evento: InjectEvent, escrita: &Arc<Mutex<Escrita>>) {
        let Some(injetor) = self.injetor.as_mut() else {
            return;
        };
        let resultado = injetor.inject(evento);
        if let Err(erro) = resultado {
            let desktop = injetor
                .desktop()
                .unwrap_or_else(|| ir_input::desktop::PADRAO.to_owned());
            // Do ponto de vista do usuário, nada aconteceu — ele não teria como saber. Por isso
            // a recusa é contada, e não só registrada ([05, §4.4](../../../docs/05-windows.md)).
            warn!(%erro, desktop, "injeção recusada");
            let protegido = ir_input::desktop::protegido(&desktop);
            let _ = enviar(
                escrita,
                &FatoDoAgente::InjecaoRecusada { desktop, protegido },
            );
        }
    }

    /// Põe o ponteiro local na posição do protocolo, convertida para pixels desta sessão.
    ///
    /// A conversão é feita **aqui**, e não no serviço: quem sabe o arranjo de telas do usuário é
    /// quem está na sessão dele. Um serviço na sessão 0 leria métricas que não são as dele. A
    /// posição é relativa ao monitor que ela nomeia — a mesma conversão da sessão
    /// ([`Desktop::from_position`]); tratá-la como fração do desktop virtual inteiro prendia o
    /// ponteiro no lugar errado com dois monitores.
    fn prender(&self, posicao: PointerPosition) {
        let (Some(capturador), Some(telas)) = (self.capturador.as_ref(), self.telas.as_ref())
        else {
            return;
        };
        let ponto = telas.from_position(posicao);
        capturador.warp_pointer(ponto.x, ponto.y);
    }

    /// Solta tudo que possa estar pressionado. O comando mais importante do produto.
    pub(crate) fn soltar_tudo(&mut self) {
        if let Some(injetor) = self.injetor.as_mut() {
            let _ = injetor.release_all();
        }
        if let Some(capturador) = self.capturador.as_ref() {
            // Se caímos com a supressão ligada, o teclado do usuário ficaria morto.
            capturador.set_suppress(false);
        }
    }
}

impl Drop for Entrada {
    fn drop(&mut self) {
        self.soltar_tudo();
    }
}
