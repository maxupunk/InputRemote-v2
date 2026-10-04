//! O recado no Linux: a notificação do sistema, pelo `notify-send`; e, nos submódulos, o ícone na
//! bandeja ([`bandeja`]) e o andamento no ícone do dock ([`doca`]).
//!
//! **A notificação conta o fim, e só o fim.** O GNOME não desenha barra de andamento na notificação,
//! e um recado de andamento que se substitui a cada passo reaparecia na tela — piscava e chamava a
//! atenção para algo que não pede nada. O andamento fica onde ele não interrompe: o ícone da bandeja
//! girando, com a frase no menu, e a barra no ícone do dock. A notificação aparece uma vez, quando a
//! cópia chegou ou não atravessou, e para o que o outro computador fez.
//!
//! Por `notify-send`, e não por D-Bus direto: é uma linha de processo contra um cliente de barramento
//! inteiro como dependência, e o pacote já exige `libnotify` por isso. Antes cada lugar que avisava
//! — o ajudante de clipboard, as pastas, a janela — montava a própria linha; agora os parâmetros
//! moram aqui, e cada um só diz o [`Recado`].
//!
//! - **"Abrir a pasta"** no que chegou (`--action`), com o arquivo já selecionado no gerenciador
//!   ([`arquivos`]). O `notify-send` espera a escolha, então quem espera é uma thread; o laço de
//!   quem avisou segue.
//! - **A imagem que chegou vai em miniatura** (`image-path`): uma captura de tela se reconhece na
//!   hora.

pub mod arquivos;
pub mod bandeja;
pub mod doca;

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use super::{Recado, Tom};

/// Mostra o recado, uma vez. Um recado de andamento não aparece: o andamento é da bandeja e do
/// dock.
///
/// Falhar aqui não é motivo para nada: o que aconteceu continua acontecendo, e o retorno vai
/// faltar só na tela.
pub fn avisar(recado: &Recado) {
    if recado.andamento.is_some() {
        return;
    }
    let mut comando = comando(recado);
    match &recado.recebido {
        // Com botão, o `notify-send` só termina quando o recado fecha: lê-se em outra thread.
        Some(recebido) => esperar_o_botao(&mut comando, recebido.clone()),
        None => {
            if let Err(erro) = comando.stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                tracing::debug!(%erro, "sem notify-send; o recado fica só na janela");
            }
        }
    }
}

/// A linha do `notify-send` para este recado.
fn comando(recado: &Recado) -> Command {
    let mut comando = Command::new("notify-send");
    let urgencia = if recado.tom == Tom::Problema {
        "critical"
    } else {
        "normal"
    };
    comando
        .arg("--app-name=InputRemote")
        .arg("--icon=inputremote")
        .arg(format!("--urgency={urgencia}"));
    if recado.recebido.is_some() {
        comando.arg("--action=abrir=Abrir a pasta");
    }
    if let Some(imagem) = recado.imagem() {
        comando.arg(format!("--hint=string:image-path:{imagem}"));
    }
    comando.arg(&recado.titulo).arg(&recado.corpo);
    comando
}

/// Mostra o recado com botão e espera a escolha numa thread: "abrir" abre a pasta, com o que
/// chegou selecionado.
fn esperar_o_botao(comando: &mut Command, recebido: String) {
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
            // O `notify-send` imprime o botão escolhido, se houver, quando o recado fecha.
            let escolheu = BufReader::new(saida)
                .lines()
                .map_while(Result::ok)
                .any(|linha| linha.trim() == "abrir");
            let _ = filho.wait();
            if escolheu {
                arquivos::mostrar(std::path::Path::new(&recebido));
            }
        });
}

/// O andamento em porcentagem inteira, para a barra do dock.
fn porcento(andamento: f32) -> u8 {
    // `clamp` garante 0..=100, que cabe em `u8` e não é NaN.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (andamento.clamp(0.0, 1.0) * 100.0).round() as u8
    }
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
    fn o_que_chegou_oferece_abrir_a_pasta_com_a_miniatura() {
        let recado = Recado {
            titulo: "Chegou".to_owned(),
            corpo: "fotos".to_owned(),
            tom: Tom::Feito,
            andamento: None,
            pasta: Some("/home/ana/Recebidos".to_owned()),
            recebido: Some("/home/ana/Recebidos/captura.png".to_owned()),
        };
        let args = linha(&comando(&recado));
        assert!(args.contains(&"--action=abrir=Abrir a pasta".to_owned()));
        assert!(
            args.contains(&"--hint=string:image-path:/home/ana/Recebidos/captura.png".to_owned()),
            "a imagem que chegou, em miniatura: {args:?}"
        );
        assert_eq!(args.last().map(String::as_str), Some("fotos"));
    }

    #[test]
    fn o_problema_e_urgente() {
        let recado = Recado::conexao_perdida("o cabo saiu");
        assert!(linha(&comando(&recado)).contains(&"--urgency=critical".to_owned()));
    }

    #[test]
    fn a_porcentagem_e_inteira_e_limitada() {
        assert_eq!(porcento(0.456), 46);
        assert_eq!(porcento(1.5), 100);
    }
}
