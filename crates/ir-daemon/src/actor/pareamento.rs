//! O prazo do pareamento: o código vale dois minutos, e depois disso o serviço desiste.
//!
//! Sem prazo, um código que ninguém confirmou deixava o serviço esperando para sempre — e, como
//! quem espera confirmação não tenta reconectar, a máquina ficava fora do ar até alguém reiniciar
//! o serviço à mão. A interface já prometia "o código vale 2 minutos"; aqui a promessa passa a ser
//! verdade.
//!
//! O mesmo vale para o enlace que cai com o código na tela: não há mais o que confirmar, e ficar
//! esperando é o mesmo defeito por outro caminho.

use std::time::{Duration, Instant};

use ir_ipc::Aviso;
use ir_net::NetCommand;
use tracing::warn;

use super::Daemon;

/// Quanto tempo o código de pareamento vale.
///
/// O mesmo número que a interface mostra ao usuário em [`ir_ipc::Falha::PareamentoExpirou`]:
/// dois números diferentes para a mesma coisa seriam uma promessa quebrada.
const PRAZO_DO_PAREAMENTO: Duration = Duration::from_secs(120);

/// Se um pareamento começado em `desde` já passou do prazo em `agora`.
///
/// Relógio monotônico, e saturando: um `agora` anterior a `desde` conta como tempo zero, e nunca
/// como código vencido.
fn expirou(desde: Instant, agora: Instant) -> bool {
    agora.saturating_duration_since(desde) >= PRAZO_DO_PAREAMENTO
}

impl Daemon {
    /// Se há um código na tela esperando a comparação.
    pub(super) const fn aguardando_confirmacao(&self) -> bool {
        self.pareamento_desde.is_some()
    }

    /// Desiste do pareamento se o código passou do prazo.
    pub(super) fn vencer_pareamento_se_preciso(&mut self) {
        let Some(desde) = self.pareamento_desde else {
            return;
        };
        if !expirou(desde, Instant::now()) {
            return;
        }
        warn!("o código de pareamento expirou sem confirmação");
        // Recusar é o que desmonta o handshake do lado da rede e avisa o outro computador, para
        // ele também sair da tela de comparação em vez de esperar o próprio prazo vencer.
        let _ = self.net.send(NetCommand::ConfirmPairing(false));
        self.encerrar_pareamento_sem_sucesso();
    }

    /// O enlace caiu com um código na tela: não há mais o que confirmar.
    pub(super) fn abandonar_pareamento_pendente(&mut self) {
        if self.aguardando_confirmacao() {
            warn!("o enlace caiu no meio do pareamento");
            self.encerrar_pareamento_sem_sucesso();
        }
    }

    /// Limpa o pareamento pendente e tira a interface da tela de comparação.
    pub(super) fn encerrar_pareamento_sem_sucesso(&mut self) {
        self.pareamento_desde = None;
        self.pending_peer = None;
        let _ = self
            .avisos
            .send(Aviso::PareamentoConcluido { sucesso: false });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn um_codigo_recem_mostrado_ainda_vale() {
        let agora = Instant::now();
        assert!(!expirou(agora, agora));
        assert!(!expirou(
            agora,
            agora + PRAZO_DO_PAREAMENTO.saturating_sub(Duration::from_secs(1))
        ));
    }

    #[test]
    fn o_codigo_vence_no_prazo_e_continua_vencido_depois() {
        let desde = Instant::now();
        assert!(expirou(desde, desde + PRAZO_DO_PAREAMENTO));
        assert!(expirou(desde, desde + PRAZO_DO_PAREAMENTO * 2));
    }

    #[test]
    fn um_agora_anterior_ao_inicio_nao_vence_o_codigo() {
        // Saturar e não subtrair: um `agora` que chega antes de `desde` não pode virar um número
        // gigante e declarar vencido um código que acabou de aparecer.
        let agora = Instant::now();
        assert!(!expirou(agora + Duration::from_secs(10), agora));
    }

    #[test]
    fn o_prazo_e_o_mesmo_que_a_interface_promete() {
        let promessa = ir_ipc::Falha::PareamentoExpirou.o_que_fazer();
        assert!(
            promessa.contains("2 minutos"),
            "a interface diz `{promessa}`, e o prazo aqui é {PRAZO_DO_PAREAMENTO:?}"
        );
        assert_eq!(PRAZO_DO_PAREAMENTO, Duration::from_secs(120));
    }
}
