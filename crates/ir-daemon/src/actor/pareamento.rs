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

use ir_ipc::{Aviso, Resposta};
use ir_session::LinkDown;
use tracing::{info, warn};

use super::Daemon;

/// Quanto tempo o código de pareamento vale.
///
/// O mesmo número que a interface mostra ao usuário em [`ir_ipc::Falha::PareamentoExpirou`]:
/// dois números diferentes para a mesma coisa seriam uma promessa quebrada.
const PRAZO_DO_PAREAMENTO: Duration = Duration::from_secs(120);

/// Um pareamento em andamento: do código na tela até o fim.
///
/// Termina com o par gravado, com o enlace caindo, com a recusa ou no prazo — e não no clique em
/// "São iguais", que só diz que deste lado confere (log 25).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pareamento {
    /// Quando o código apareceu. O prazo conta daqui.
    pub(super) desde: Instant,
    /// Se o usuário já disse que o código confere, e só falta o outro computador.
    pub(super) conferido: bool,
    /// Os seis dígitos, para a janela que abrir depois também recebê-los.
    pub(super) digitos: [u8; 6],
    /// A chave que o par apresentou, e que só vai ser gravada com o fim do pareamento.
    ///
    /// Aqui, e não num campo solto do ator: eram dois campos para o mesmo pareamento, e um podia
    /// ficar para trás quando o outro era limpo.
    pub(super) chave: ir_crypto::PublicKey,
}

/// Se um pareamento começado em `desde` já passou do prazo em `agora`.
///
/// Relógio monotônico, e saturando: um `agora` anterior a `desde` conta como tempo zero, e nunca
/// como código vencido.
fn expirou(desde: Instant, agora: Instant) -> bool {
    agora.saturating_duration_since(desde) >= PRAZO_DO_PAREAMENTO
}

impl Daemon {
    /// Se há um código na tela esperando a comparação do usuário.
    pub(super) const fn aguardando_confirmacao(&self) -> bool {
        matches!(
            self.pareamento,
            Some(Pareamento {
                conferido: false,
                ..
            })
        )
    }

    /// Se há um pareamento em andamento: do código na tela até o fim, com ou sem a resposta do
    /// usuário.
    ///
    /// Não é o mesmo que [`Self::aguardando_confirmacao`]. Depois de "São iguais" o pareamento
    /// ainda espera o outro computador, e tratá-lo como terminado deixava a reconexão discar por
    /// cima e a janela sem saber que não deu (log 25).
    pub(super) const fn pareando(&self) -> bool {
        self.pareamento.is_some()
    }

    /// Desiste do pareamento se ele passou do prazo.
    pub(super) fn vencer_pareamento_se_preciso(&mut self) {
        let Some(pareamento) = self.pareamento else {
            return;
        };
        if !expirou(pareamento.desde, Instant::now()) {
            return;
        }
        if let Some(transporte) = self.transporte_do_par() {
            if pareamento.conferido {
                // O usuário já disse que confere, e é o outro lado que não respondeu. Recusar
                // agora mandaria "códigos diferentes" — o sinal de alguém no meio — por um motivo
                // que não é esse. Desfazer o enlace basta.
                warn!("o pareamento conferido não terminou no prazo");
                transporte.desconectar();
            } else {
                warn!("o código de pareamento expirou sem confirmação");
                // Recusar é o que desmonta o handshake do lado do transporte e avisa o outro
                // computador, para ele também sair da tela de comparação em vez de esperar o
                // próprio prazo vencer.
                transporte.confirmar_pareamento(false);
            }
        }
        self.encerrar_pareamento_sem_sucesso();
    }

    /// O enlace caiu no meio do pareamento: não há mais o que confirmar, nem o que esperar.
    pub(super) fn abandonar_pareamento_pendente(&mut self) {
        if self.pareando() {
            warn!("o enlace caiu no meio do pareamento");
            self.encerrar_pareamento_sem_sucesso();
        }
    }

    /// Conta de novo o código em comparação, para uma janela que acabou de começar a acompanhar.
    ///
    /// Sem isto, o código ia por aviso uma vez só: a janela aberta depois dele — ou tirada da
    /// bandeja — ficava sem nada para comparar enquanto a outra tela mostrava os dígitos.
    pub(super) fn recontar_codigo_pendente(&self) {
        if let Some(pareamento) = self.pareamento.filter(|p| !p.conferido) {
            let _ = self.avisos.send(Aviso::CodigoDePareamento {
                digitos: pareamento.digitos,
            });
        }
    }

    /// Limpa o pareamento pendente e tira a interface da tela de comparação.
    pub(super) fn encerrar_pareamento_sem_sucesso(&mut self) {
        self.pareamento = None;
        let _ = self
            .avisos
            .send(Aviso::PareamentoConcluido { sucesso: false });
    }

    /// Esquece o par gravado, e desliga dele na hora.
    ///
    /// Só apagar a chave deixava o enlace e a sessão de pé: a janela seguia em "Conectando…" e o
    /// serviço continuava tentando (log 25). O endereço fica, porque é o candidato que "Procurar"
    /// oferece para parear de novo — e, sem par gravado, ninguém disca para ele sozinho.
    pub(super) fn esquecer_par(&mut self) -> Resposta {
        let resposta = self.persistir_com(|config| config.peers.clear());
        if resposta != Resposta::Feito {
            return resposta;
        }
        info!("par esquecido pela interface; conexão encerrada");
        // Sem par, sem permissão: o agente, a tela protegida e a política do Windows acompanham.
        self.permissao_do_protegido_mudou();
        // O canal de arquivos com ele cai também: esquecido, ele não recebe mais nada daqui.
        self.atualizar_destino_dos_arquivos();
        if self.pareando() {
            self.encerrar_pareamento_sem_sucesso();
        }
        self.despedir_e_derrubar(LinkDown::UserStopped);
        resposta
    }
}

impl Daemon {
    pub(crate) fn on_pairing_code(&mut self, code: [u8; 6], peer_static: ir_crypto::PublicKey) {
        self.discagem_atendida();
        self.pareamento = Some(Pareamento {
            desde: Instant::now(),
            conferido: false,
            digitos: code,
            chave: peer_static,
        });
        let digits: String = code.iter().map(|d| char::from(b'0' + d)).collect();
        // O código fica fora do registro: quem lê o registro depois não precisa dele, e ele vale
        // enquanto a comparação estiver aberta.
        info!("código de pareamento na tela; esperando a comparação");
        println!("\n=== CÓDIGO DE PAREAMENTO: {digits} ===");
        println!("Confere com o outro computador? [s/n] e Enter:");
        // A interface mostra os seis dígitos em caixas para a comparação em voz alta; vão
        // separados, não como texto, exatamente por isso.
        let _ = self
            .avisos
            .send(Aviso::CodigoDePareamento { digitos: code });
    }

    pub(super) fn on_confirm(&mut self, line: &str) {
        let trimmed = line.trim();
        if !self.aguardando_confirmacao() || trimmed.is_empty() {
            // Uma linha vazia (Enter solto) não é resposta: ignorar, não recusar.
            return;
        }
        let yes = matches!(trimmed.to_lowercase().as_str(), "s" | "sim" | "y" | "yes");
        let _ = self.confirmar(yes);
    }

    /// A resposta à comparação do código, venha do terminal ou da interface.
    ///
    /// Devolve se havia código esperando a resposta. Um clique num código que já não vale precisa
    /// virar explicação na janela, e não um "feito" que não muda nada (log 25).
    pub(crate) fn confirmar(&mut self, yes: bool) -> bool {
        if !self.aguardando_confirmacao() {
            return false;
        }
        info!("confirmação recebida: {}", if yes { "sim" } else { "não" });
        if let Some(transporte) = self.transporte_do_par() {
            transporte.confirmar_pareamento(yes);
        }
        if yes {
            // Deste lado confere, mas o pareamento só termina quando o outro lado também
            // confirmar. Até lá ele segue em andamento: com prazo, sem rediscagem por cima, e com
            // a janela avisada se não chegar ao fim.
            if let Some(pareamento) = self.pareamento.as_mut() {
                pareamento.conferido = true;
            }
        } else {
            // Códigos diferentes ou recusa: não há par, e a interface precisa saber que o
            // pareamento terminou sem sucesso para sair da tela de comparação.
            self.encerrar_pareamento_sem_sucesso();
        }
        true
    }
}

#[cfg(test)]
mod tests;
