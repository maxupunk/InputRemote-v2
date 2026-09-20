//! A notificação do sistema, no Linux.
//!
//! No Windows a interface fica na bandeja e mostra um aviso no canto da tela. No GNOME não há
//! bandeja, e a janela costuma estar fechada — quem copia está no Nautilus. O lugar certo do
//! recado ali é a notificação do sistema, e quem sempre está de pé na sessão é este ajudante.
//!
//! Por `notify-send`, e não por D-Bus direto: é uma linha de processo contra um cliente de barramento
//! inteiro como dependência, e o pacote já exige `libnotify` por isso.
//!
//! Só duas notificações por cópia — o começo e o fim. Uma por quadro de progresso viraria ruído,
//! e ruído é o que se aprende a ignorar.

use ir_ipc::transferencia::Transferencia;

/// Guarda o que já foi dito, para não repetir.
#[derive(Debug, Default)]
pub(crate) struct Notificador {
    ultima: Option<String>,
}

impl Notificador {
    /// Conta esta cópia ao usuário, se ela for novidade.
    pub(crate) fn contar(&mut self, copia: &Transferencia) {
        let chave = chave(copia);
        if self.ultima.as_deref() == Some(chave.as_str()) {
            return;
        }
        self.ultima = Some(chave);
        enviar(copia);
    }
}

/// O que distingue uma notificação da seguinte: o item e se ele terminou.
///
/// O progresso não entra de propósito: é o que faria a mesma cópia notificar dez vezes.
fn chave(copia: &Transferencia) -> String {
    format!(
        "{}|{}|{}",
        copia.nome,
        copia.sentido.rotulo(),
        copia.terminou()
    )
}

/// Manda a notificação. Falhar aqui não é motivo para nada: a cópia continua.
#[cfg(target_os = "linux")]
fn enviar(copia: &Transferencia) {
    use std::process::{Command, Stdio};

    let urgencia = if copia.falhou() { "critical" } else { "normal" };
    let resultado = Command::new("notify-send")
        .arg("--app-name=InputRemote")
        .arg("--icon=inputremote")
        .arg(format!("--urgency={urgencia}"))
        // Substitui a notificação anterior desta mesma cópia em vez de empilhar outra: é o que o
        // GNOME entende por "atualize aquele recado".
        .arg("--hint=string:x-canonical-private-synchronous:inputremote-copia")
        .arg(copia.titulo())
        .arg(copia.detalhe())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(erro) = resultado {
        tracing::debug!(%erro, "sem notify-send; a cópia continua, sem recado na tela");
    }
}

/// Fora do Linux quem conta é a interface, com o aviso no canto da tela.
#[cfg(not(target_os = "linux"))]
fn enviar(_copia: &Transferencia) {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use ir_ipc::transferencia::{Fase, Motivo, Sentido};

    use super::*;

    fn copia(nome: &str, fase: Fase) -> Transferencia {
        Transferencia {
            sentido: Sentido::Recebendo,
            nome: nome.to_owned(),
            bytes_feitos: 1,
            bytes_total: 2,
            fase,
        }
    }

    #[test]
    fn o_progresso_nao_vira_uma_notificacao_por_quadro() {
        let andando = copia("pasta-B", Fase::Andando);
        let mut outro = andando.clone();
        outro.bytes_feitos = 2;
        assert_eq!(chave(&andando), chave(&outro), "só o progresso mudou");
    }

    #[test]
    fn o_comeco_e_o_fim_sao_recados_diferentes() {
        let andando = chave(&copia("pasta-B", Fase::Andando));
        let pronta = chave(&copia(
            "pasta-B",
            Fase::Concluida {
                destino: "/tmp/pasta-B".to_owned(),
            },
        ));
        let parada = chave(&copia("pasta-B", Fase::Parada(Motivo::CanalCaiu)));
        assert_ne!(andando, pronta);
        assert_eq!(pronta, parada, "os dois fins fecham a mesma cópia");
    }

    #[test]
    fn cada_item_tem_o_seu_recado() {
        assert_ne!(
            chave(&copia("pasta-A", Fase::Andando)),
            chave(&copia("pasta-B", Fase::Andando))
        );
    }

    #[test]
    fn a_mesma_coisa_nao_e_dita_duas_vezes() {
        let mut notificador = Notificador::default();
        let copia = copia("pasta-B", Fase::Andando);
        notificador.contar(&copia);
        let primeira = notificador.ultima.clone();
        notificador.contar(&copia);
        assert_eq!(notificador.ultima, primeira);
    }
}
