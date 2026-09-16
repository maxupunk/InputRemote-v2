//! O despacho dos comandos que a sessão produz.
//!
//! A sessão é pura: ela devolve uma lista de comandos, e é aqui que cada um vira efeito no mundo
//! — um quadro no socket, uma injeção, a supressão da entrada local. É a periferia de
//! [02, §4](../../../docs/02-arquitetura.md).

use ir_input::InjectEvent;
use ir_ipc::ComandoDoAgente;
use ir_proto::carrier::Carrier;
use ir_session::{Command, Injection, Notice};
use tracing::{debug, info, warn};

use crate::actor::Daemon;

impl Daemon {
    /// Executa os comandos acumulados no último passo e esvazia o lote.
    pub(crate) fn apply_commands(&mut self) {
        let commands: Vec<Command> = self.out.iter().cloned().collect();
        self.out.clear();
        for command in commands {
            self.apply_one(command);
        }
    }

    fn apply_one(&mut self, command: Command) {
        match command {
            Command::Send { carrier, frame } => self.send_frame(carrier, &frame),
            Command::Inject(injection) => self.inject(injection),
            Command::ReleaseAll => self.release_all(),
            Command::SuppressLocalInput(on) => self.suppress(on),
            Command::WarpPointer(position) => self.warp(position),
            Command::Notify(notice) => {
                log_notice(&notice);
                // A borda que a sessão passou a usar precisa ir para o arquivo e para a janela.
                if let Notice::EdgeChanged { edge } = notice {
                    self.adotar_borda(edge);
                }
            }
            // Os temporizadores são otimização (o serviço bate a sessão periodicamente e ela
            // confere os próprios prazos pelo relógio injetado); o curinga cobre variantes
            // futuras do enum não exaustivo.
            _ => {}
        }
    }

    /// Manda o quadro **pelo portador que a sessão escolheu**.
    ///
    /// Este método recebia o portador e o ignorava, mandando tudo para o socket de rede. Com um
    /// transporte só isso não aparecia; com dois, a tela diria "Bluetooth" e os bytes iriam pela
    /// rede — e o limite de tamanho conferido na codificação seria o do portador errado, já que
    /// o teto do rádio é menor que o da rede ([03, §2](../../../docs/03-protocolo.md)).
    fn send_frame(&self, carrier: Carrier, frame: &ir_proto::frame::Frame) {
        let Some(transporte) = self.transporte(carrier) else {
            // A sessão só escolhe portador que ela declarou disponível, então chegar aqui é
            // defeito nosso — e vale dizer, em vez de o quadro sumir em silêncio.
            warn!(%carrier, "a sessão pediu um portador que não está aberto");
            return;
        };
        match ir_proto::codec::encode(frame, carrier) {
            Ok(bytes) => transporte.enviar(bytes),
            Err(error) => warn!(%error, %carrier, "não foi possível codificar o quadro"),
        }
    }

    fn inject(&mut self, injection: Injection) {
        // Com agente de pé (o caso do Windows), quem toca no teclado é ele: o serviço está na
        // sessão 0 e o `SendInput` dele não chegaria ao desktop de ninguém.
        if let Some(agente) = self.comandos_do_agente() {
            if let Some(comando) = to_agent_command(injection) {
                let _ = agente.send(comando);
            }
            return;
        }
        let Some(injector) = self.injector.as_mut() else {
            return; // o servidor não injeta
        };
        let Some(event) = to_inject_event(injection) else {
            return;
        };
        if let Err(error) = injector.inject(event) {
            // Recusa é o sintoma do endurecimento no Windows; não derruba a sessão sozinha.
            debug!(%error, "injeção recusada");
        }
    }

    fn release_all(&mut self) {
        if let Some(agente) = self.comandos_do_agente() {
            let _ = agente.send(ComandoDoAgente::SoltarTudo);
            return;
        }
        if let Some(injector) = self.injector.as_mut() {
            let _ = injector.release_all();
        }
    }

    fn suppress(&self, on: bool) {
        if let Some(agente) = self.comandos_do_agente() {
            let _ = agente.send(ComandoDoAgente::SuprimirEntradaLocal(on));
            return;
        }
        if let Some(capturer) = self.capturer.as_ref() {
            capturer.set_suppress(on);
        }
    }

    fn warp(&self, position: ir_proto::input::PointerPosition) {
        if let Some(agente) = self.comandos_do_agente() {
            // A posição vai **normalizada**, e quem a converte em pixels é o agente: o tamanho
            // da tela do usuário só é conhecido de dentro da sessão dele.
            let _ = agente.send(ComandoDoAgente::PrenderPonteiro(position));
            return;
        }
        if let Some(capturer) = self.capturer.as_ref() {
            let (w, h) = self.screen;
            let x = i32::try_from(u32::from(position.x) * w / 65_535).unwrap_or(0);
            let y = i32::try_from(u32::from(position.y) * h / 65_535).unwrap_or(0);
            capturer.warp_pointer(x, y);
        }
    }
}

/// Converte um comando de injeção da sessão no comando que o agente entende.
fn to_agent_command(injection: Injection) -> Option<ComandoDoAgente> {
    Some(match injection {
        Injection::Key { usage, pressed } => ComandoDoAgente::Tecla {
            usage,
            pressionada: pressed,
        },
        Injection::Button { button, pressed } => ComandoDoAgente::Botao {
            botao: button,
            pressionado: pressed,
        },
        Injection::Wheel(delta) => ComandoDoAgente::Roda(delta),
        Injection::Pointer(position) => ComandoDoAgente::Ponteiro(position),
        _ => return None,
    })
}

/// Converte um comando de injeção da sessão no evento do backend de entrada.
fn to_inject_event(injection: Injection) -> Option<InjectEvent> {
    Some(match injection {
        Injection::Key { usage, pressed } => InjectEvent::Key { usage, pressed },
        Injection::Button { button, pressed } => InjectEvent::Button { button, pressed },
        Injection::Wheel(delta) => InjectEvent::Wheel(delta),
        Injection::Pointer(position) => InjectEvent::Pointer(position),
        _ => return None,
    })
}

/// Registra um aviso da sessão. Nunca inclui conteúdo digitado
/// ([04, §7](../../../docs/04-seguranca.md)).
fn log_notice(notice: &Notice) {
    match notice {
        Notice::Connected { peer, carrier } => info!(%peer, %carrier, "sessão estabelecida"),
        Notice::Disconnected { reason, will_retry } => {
            info!(?reason, will_retry, "sessão encerrada");
        }
        Notice::ControlMoved { remote } => info!(remote, "controle mudou de lado"),
        Notice::CarrierChanged { carrier, why } => info!(%carrier, ?why, "portador escolhido"),
        Notice::Reconciled { released, pressed } => {
            debug!(released, pressed, "estado reconciliado");
        }
        Notice::LatencySample(rtt) => debug!(%rtt, "latência medida"),
        Notice::ProtocolError { code, fatal } => warn!(?code, fatal, "erro de protocolo"),
        Notice::EdgeChanged { edge } => debug!(%edge, "borda em uso"),
        _ => {}
    }
}
