//! O que o agente faz na máquina local a mando do serviço: injetar, suprimir, prender e soltar.

use std::sync::{Arc, Mutex};

use ir_input::{Capturer, InjectEvent, Injector};
use ir_ipc::{ComandoDoAgente, FatoDoAgente};
use ir_proto::input::PointerPosition;
use tracing::warn;

use crate::{Escrita, enviar};

/// O que o agente tem para mexer na máquina local.
pub(crate) struct Entrada {
    pub(crate) injetor: Option<Box<dyn Injector>>,
    pub(crate) capturador: Option<Box<dyn Capturer>>,
}

impl Entrada {
    /// Executa um comando do serviço.
    pub(crate) fn executar(&mut self, comando: ComandoDoAgente, escrita: &Arc<Mutex<Escrita>>) {
        match comando {
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
            outro => self.injetar(outro, escrita),
        }
    }

    /// Injeta o que o comando pedir, e conta ao serviço se o sistema recusar.
    fn injetar(&mut self, comando: ComandoDoAgente, escrita: &Arc<Mutex<Escrita>>) {
        let Some(evento) = para_evento(comando) else {
            return;
        };
        let Some(injetor) = self.injetor.as_mut() else {
            return;
        };
        let resultado = injetor.inject(evento);
        let desktop = injetor.desktop().unwrap_or_else(|| "Default".to_owned());
        if let Err(erro) = resultado {
            // Do ponto de vista do usuário, nada aconteceu — ele não teria como saber. Por isso
            // a recusa é contada, e não só registrada ([05, §4.4](../../../docs/05-windows.md)).
            warn!(%erro, desktop, "injeção recusada");
            let _ = enviar(escrita, &FatoDoAgente::InjecaoRecusada { desktop });
        }
    }

    /// Põe o ponteiro local na posição normalizada, convertida para pixels desta tela.
    ///
    /// A conversão é feita **aqui**, e não no serviço: quem sabe o tamanho da tela do usuário é
    /// quem está na sessão dele. Um serviço na sessão 0 leria métricas que não são as dele.
    fn prender(&self, posicao: PointerPosition) {
        let Some(capturador) = self.capturador.as_ref() else {
            return;
        };
        // A posição normalizada é sobre o desktop virtual inteiro, como a injeção: com dois monitores,
        // converter pela tela principal prendia o ponteiro no lugar errado.
        let (origem_x, origem_y, largura, altura) = ir_input::desktop_virtual()
            .or_else(|| ir_input::primary_screen_size().map(|(l, a)| (0, 0, l, a)))
            .unwrap_or((0, 0, 1920, 1080));
        let escala = |valor: u16, tamanho: u32| {
            i32::try_from(u64::from(valor) * u64::from(tamanho) / 65_535).unwrap_or(0)
        };
        let x = origem_x.saturating_add(escala(posicao.x, largura));
        let y = origem_y.saturating_add(escala(posicao.y, altura));
        capturador.warp_pointer(x, y);
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

/// Converte um comando de injeção no evento do backend de entrada.
fn para_evento(comando: ComandoDoAgente) -> Option<InjectEvent> {
    Some(match comando {
        ComandoDoAgente::Tecla { usage, pressionada } => InjectEvent::Key {
            usage,
            pressed: pressionada,
        },
        ComandoDoAgente::Botao { botao, pressionado } => InjectEvent::Button {
            button: botao,
            pressed: pressionado,
        },
        ComandoDoAgente::Roda(delta) => InjectEvent::Wheel(delta),
        ComandoDoAgente::Ponteiro(posicao) => InjectEvent::Pointer(posicao),
        _ => return None,
    })
}
