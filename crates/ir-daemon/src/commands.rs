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
    ///
    /// O lote sai do ator enquanto os comandos são aplicados — um comando pode alimentar a sessão
    /// de novo, e o passo novo usa um lote próprio. Antes cada passo clonava todos os comandos, 200
    /// vezes por segundo; agora eles saem por valor, e a capacidade reservada volta para o próximo.
    pub(crate) fn apply_commands(&mut self) {
        let mut lote = std::mem::take(&mut self.out);
        for command in lote.drain() {
            self.apply_one(command);
        }
        if self.out.is_empty() {
            self.out = lote;
        }
    }

    fn apply_one(&mut self, command: Command) {
        match command {
            Command::Send { carrier, frame } => self.send_frame(carrier, &frame),
            Command::Inject(injection) => self.inject(injection),
            Command::ReleaseAll => self.release_all(),
            Command::SuppressLocalInput(on) => self.suppress(on),
            Command::WarpPointer(position) => self.warp(position),
            Command::SecureAttention => self.gerar_ctrl_alt_del(),
            Command::LockScreen => self.bloquear_a_tela(),
            Command::Notify(notice) => self.on_notice(&notice),
            // Chegou texto do par: vai para o ajudante da sessão, que o põe no clipboard. Os dois
            // tipos têm o mesmo limite, o do canal 4.
            Command::ClipboardText(texto) => {
                debug!(
                    bytes = texto.as_str().len(),
                    "texto de clipboard recebido do par"
                );
                if let Some(texto) = ir_ipc::TextoDoClipboard::novo(texto.into_string()) {
                    let _ = self.avisos.send(ir_ipc::Aviso::TextoRecebido(texto));
                }
            }
            // Os temporizadores são otimização (o serviço bate a sessão periodicamente e ela
            // confere os próprios prazos pelo relógio injetado); o curinga cobre variantes
            // futuras do enum não exaustivo.
            _ => {}
        }
    }

    /// Um aviso da sessão: registrar, e o que ele muda no serviço e na janela.
    fn on_notice(&mut self, notice: &Notice) {
        log_notice(notice);
        // A borda que o par escolheu precisa ir para o arquivo e para a janela; a escolhida aqui
        // já foi gravada antes de chegar à sessão.
        if let Notice::EdgeAdopted { edge, chosen_at } = notice {
            self.adotar_borda(*edge, *chosen_at);
        }
        // O controle saiu desta máquina: o que está no clipboard daqui vai junto. É o
        // gatilho que funciona onde o sistema não avisa mudança de clipboard — o GNOME não
        // avisa (ADR-0011).
        if let Notice::ControlMoved { remote: true } = notice {
            let _ = self.avisos.send(ir_ipc::Aviso::LerClipboard);
        }
        // O rádio do par forma a rota dupla quando os dois se conheceram pela rede.
        if let Notice::PeerRadio(radio) = notice {
            self.on_radio_do_par(*radio);
        }
        // A rota mudou sem a fase mudar, e o aviso de fase não acordaria a janela.
        match notice {
            Notice::PeerNetworkPower(estado) => self.on_economia_do_par(Some(*estado)),
            Notice::NetworkPowerFixRequested => {
                self.on_pedido_de_economia_do_par(std::time::Instant::now());
            }
            // O que o par contou vale para a sessão dele; na próxima, ele conta de novo.
            Notice::Disconnected { reason, .. } => {
                self.on_economia_do_par(None);
                self.on_queda(*reason);
            }
            Notice::LatencySample(volta) => {
                self.voltas.anotar(std::time::Instant::now(), volta.get());
            }
            Notice::Connected { peer, .. } => {
                // A sessão de pé é o par compartilhando: uma pausa dele que ficou marcada acabou.
                self.par_retomou();
                self.lembrar_nome_do_par(peer.as_str());
            }
            Notice::PeerCannotSecureAttention => self.ctrl_alt_del_nao_saiu(),
            Notice::PeerProtectedDesktop { refused } => {
                self.on_par_recusa_protegido(*refused);
            }
            _ => {}
        }
        if let Notice::RouteChanged { .. } = notice {
            let _ = self.avisos.send(ir_ipc::Aviso::EstadoMudou(self.estado()));
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
            if let Some(comando) = ir_painel::comando_do_agente(injection) {
                let _ = agente.send(comando);
            }
            return;
        }
        if self.barrar_no_protegido(injection) {
            return;
        }
        self.injetar_direto(injection);
    }

    /// Injeta pelo backend local — o `uinput` do Linux —, sem passar pela política.
    pub(crate) fn injetar_direto(&mut self, injection: Injection) {
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
        if let Some(injector) = self.injector.as_mut()
            && let Err(erro) = injector.release_all()
        {
            // Soltar tudo é o comando mais importante do produto: falhar nele é tecla presa, e isso
            // não pode sumir sem rastro.
            tracing::error!(%erro, "não foi possível soltar as teclas injetadas");
        }
    }

    fn suppress(&mut self, on: bool) {
        self.suprimindo = on;
        if let Some(agente) = self.comandos_do_agente() {
            let _ = agente.send(ComandoDoAgente::SuprimirEntradaLocal(on));
            return;
        }
        if let Some(capturer) = self.capturer.as_ref() {
            capturer.set_suppress(on);
        }
    }

    fn warp(&mut self, position: ir_proto::input::PointerPosition) {
        // No Linux conduzindo, a sessão já pôs o modelo na posição; o cursor real vai junto.
        if self.levar_cursor() {
            return;
        }
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
        Notice::RouteChanged { route, why } => info!(%route, ?why, "rota da sessão mudou"),
        Notice::Reconciled { released, pressed } => {
            debug!(released, pressed, "estado reconciliado");
        }
        Notice::LatencySample(rtt) => debug!(%rtt, "latência medida"),
        Notice::ProtocolError { code, fatal } => warn!(?code, fatal, "erro de protocolo"),
        Notice::EdgeChanged { edge, .. } => debug!(%edge, "borda escolhida aqui"),
        Notice::ControlReclaimed { here } => info!(here, "controle retomado sem atravessar"),
        _ => {}
    }
}
