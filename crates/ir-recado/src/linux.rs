//! O recado no Linux: a notificação do sistema, pelo `notify-send`.
//!
//! Por `notify-send`, e não por D-Bus direto: é uma linha de processo contra um cliente de barramento
//! inteiro como dependência, e o pacote já exige `libnotify` por isso. Antes cada lugar que avisava
//! — o ajudante de clipboard, as pastas, a janela — montava a própria linha; agora os parâmetros
//! moram aqui, e cada um só diz o [`Recado`].
//!
//! - **Um recado que se atualiza**, e não uma pilha: o primeiro guarda o id que o servidor devolve
//!   (`--print-id`) e os seguintes o substituem (`--replace-id`).
//! - **O andamento é passageiro** (`--transient`): não enche o histórico do GNOME com um "copiando
//!   12%" por cópia. O fim fica.
//! - **O andamento vai como dica** (`value`): quem sabe desenhar barra — o KDE, o XFCE — desenha.
//! - **"Abrir a pasta"** no que chegou (`--action`). O `notify-send` espera a escolha, então quem
//!   espera é uma thread; o laço de quem avisou segue.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use super::{Recado, Tom};

/// Mostra recados, atualizando o último no lugar quando pedido.
#[derive(Debug, Default)]
pub struct NotifySend {
    /// O id do último recado no servidor de notificações.
    id: Option<u32>,
}

impl NotifySend {
    /// Mostra o recado. Com `substituir`, ele toma o lugar do último que este mostrador mostrou.
    ///
    /// Falhar aqui não é motivo para nada: o que aconteceu continua acontecendo, e o retorno vai
    /// faltar só na tela.
    pub fn mostrar(&mut self, recado: &Recado, substituir: bool) {
        let substituir = if substituir { self.id } else { None };
        let mut comando = comando(recado, substituir);
        self.id = match &recado.pasta {
            // Com botão, o `notify-send` só termina quando o recado fecha: lê-se em outra thread, e
            // não há o que atualizar depois — um recado com botão é sempre o último de uma cópia.
            Some(pasta) => {
                esperar_o_botao(&mut comando, pasta.clone());
                None
            }
            None => match comando.stderr(Stdio::null()).output() {
                Ok(saida) => ler_id(&String::from_utf8_lossy(&saida.stdout)),
                Err(erro) => {
                    tracing::debug!(%erro, "sem notify-send; o recado fica só na janela");
                    None
                }
            },
        };
    }
}

/// Mostra um recado avulso, que não será atualizado.
pub fn avisar(recado: &Recado) {
    NotifySend::default().mostrar(recado, false);
}

/// A linha do `notify-send` para este recado.
fn comando(recado: &Recado, substituir: Option<u32>) -> Command {
    let mut comando = Command::new("notify-send");
    let urgencia = if recado.tom == Tom::Problema {
        "critical"
    } else {
        "normal"
    };
    comando
        .arg("--app-name=InputRemote")
        .arg("--icon=inputremote")
        .arg(format!("--urgency={urgencia}"))
        .arg("--print-id");
    if let Some(andamento) = recado.andamento {
        comando
            .arg(format!("--hint=int:value:{}", porcento(andamento)))
            .arg("--transient");
    }
    if let Some(id) = substituir {
        comando.arg(format!("--replace-id={id}"));
    }
    if recado.pasta.is_some() {
        comando.arg("--action=abrir=Abrir a pasta");
    }
    comando.arg(&recado.titulo).arg(&recado.corpo);
    comando
}

/// Mostra o recado com botão e espera a escolha numa thread: "abrir" abre a pasta.
fn esperar_o_botao(comando: &mut Command, pasta: String) {
    let filho = comando.stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let Ok(mut filho) = filho else {
        tracing::debug!("sem notify-send; o recado fica só na janela");
        return;
    };
    let Some(saida) = filho.stdout.take() else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("recado".to_owned())
        .spawn(move || {
            // A primeira linha é o id; a seguinte, se houver, é o botão escolhido.
            let escolheu = BufReader::new(saida)
                .lines()
                .map_while(Result::ok)
                .any(|linha| linha.trim() == "abrir");
            let _ = filho.wait();
            if escolheu {
                let _ = Command::new("xdg-open").arg(&pasta).spawn();
            }
        });
}

/// O andamento em porcentagem inteira, como a dica `value` pede.
fn porcento(andamento: f32) -> u8 {
    // `clamp` garante 0..=100, que cabe em `u8` e não é NaN.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (andamento.clamp(0.0, 1.0) * 100.0).round() as u8
    }
}

/// O id que o `notify-send --print-id` imprime. Sem id, a próxima atualização vira outro recado —
/// feio, e não motivo para deixar de contar.
fn ler_id(saida: &str) -> Option<u32> {
    saida.trim().lines().next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linha(comando: &Command) -> Vec<String> {
        comando
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn o_id_impresso_pelo_notify_send_e_lido() {
        assert_eq!(ler_id("42\n"), Some(42));
        assert_eq!(ler_id(" 7 "), Some(7));
        assert_eq!(ler_id(""), None);
        assert_eq!(ler_id("nada disso"), None);
    }

    #[test]
    fn o_andamento_e_passageiro_e_leva_a_porcentagem() {
        let recado = Recado {
            titulo: "Copiando".to_owned(),
            corpo: "fotos".to_owned(),
            tom: Tom::Andamento,
            andamento: Some(0.456),
            pasta: None,
        };
        let args = linha(&comando(&recado, Some(9)));
        assert!(args.contains(&"--transient".to_owned()), "{args:?}");
        assert!(args.contains(&"--hint=int:value:46".to_owned()), "{args:?}");
        assert!(args.contains(&"--replace-id=9".to_owned()), "{args:?}");
        assert_eq!(args.last().map(String::as_str), Some("fotos"));
    }

    #[test]
    fn o_que_chegou_oferece_abrir_a_pasta_e_fica_no_historico() {
        let recado = Recado {
            titulo: "Chegou".to_owned(),
            corpo: "fotos".to_owned(),
            tom: Tom::Feito,
            andamento: None,
            pasta: Some("/home/ana/Recebidos".to_owned()),
        };
        let args = linha(&comando(&recado, None));
        assert!(args.contains(&"--action=abrir=Abrir a pasta".to_owned()));
        assert!(!args.contains(&"--transient".to_owned()));
    }

    #[test]
    fn o_problema_e_urgente() {
        let recado = Recado::conexao_perdida("o cabo saiu");
        assert!(linha(&comando(&recado, None)).contains(&"--urgency=critical".to_owned()));
    }
}
