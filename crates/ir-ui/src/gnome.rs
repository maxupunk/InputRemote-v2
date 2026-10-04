//! O ícone na barra superior do GNOME: a extensão `AppIndicator`.
//!
//! O GNOME esconde os ícones de programas na barra superior. É a extensão `AppIndicator` que os
//! mostra — o Ubuntu a liga de fábrica; o Fedora a instala (o pacote do InputRemote a recomenda),
//! mas desligada. Sem ela, o ícone que gira enquanto algo atravessa fica pronto e ninguém vê.
//!
//! Por isso as Preferências oferecem ligá-la com um clique, só quando faz sentido: GNOME, extensão
//! instalada e desligada. Ligada, o ícone aparece na hora — o ajudante de clipboard já o deixou
//! esperando por ela. Sem a extensão instalada, a frase diz o pacote.

// Fora do Linux, o módulo só existe para os testes da decisão (`situacao_de`).
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::process::Command;

use slint::ComponentHandle;

use crate::gerado::{Acoes, Dados, Janela};

/// As extensões que mostram os ícones: a do Fedora e do resto, e a que o Ubuntu traz.
const EXTENSOES: [&str; 2] = [
    "appindicatorsupport@rgcjonas.gmail.com",
    "ubuntu-appindicators@ubuntu.com",
];

/// Não é GNOME, ou o ícone já aparece: o cartão some.
const NAO_SE_APLICA: i32 = 0;
/// A extensão está instalada e desligada: o cartão oferece "Ligar".
const DESLIGADA: i32 = 1;
/// A extensão não está instalada: o cartão diz o pacote.
const AUSENTE: i32 = 2;

/// Liga o cartão das Preferências: descobre a situação sem travar a abertura, e atende "Ligar".
pub(crate) fn ligar_a_tela(janela: &Janela) {
    let fraca = janela.as_weak();
    janela
        .global::<Acoes>()
        .on_ligar_bandeja_do_gnome(move || em_segundo_plano(fraca.clone(), ligar));
    em_segundo_plano(janela.as_weak(), situacao);
}

/// Pergunta ao GNOME — o que leva alguns décimos de segundo — fora do laço da janela, e põe a
/// resposta no cartão.
fn em_segundo_plano(janela: slint::Weak<Janela>, perguntar: fn() -> i32) {
    std::thread::spawn(move || {
        let situacao = perguntar();
        let _ = janela.upgrade_in_event_loop(move |janela| {
            janela.global::<Dados>().set_bandeja_do_gnome(situacao);
        });
    });
}

/// Como está o ícone na barra superior.
fn situacao() -> i32 {
    let gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|mesa| mesa.contains("GNOME"));
    if !gnome {
        return NAO_SE_APLICA;
    }
    match (listar(&["list", "--enabled"]), listar(&["list"])) {
        (Some(ligadas), Some(instaladas)) => situacao_de(&ligadas, &instaladas),
        _ => NAO_SE_APLICA,
    }
}

/// Liga a extensão e devolve a situação depois.
fn ligar() -> i32 {
    if let Err(erro) = Command::new("gnome-extensions")
        .args(["enable", EXTENSOES[0]])
        .status()
    {
        eprintln!("não consegui ligar a extensão AppIndicator: {erro}");
    }
    situacao()
}

/// A situação pelas duas listas do `gnome-extensions`, uma extensão por linha.
fn situacao_de(ligadas: &str, instaladas: &str) -> i32 {
    let tem = |lista: &str| lista.lines().any(|linha| EXTENSOES.contains(&linha.trim()));
    if tem(ligadas) {
        NAO_SE_APLICA
    } else if tem(instaladas) {
        DESLIGADA
    } else {
        AUSENTE
    }
}

fn listar(argumentos: &[&str]) -> Option<String> {
    let saida = Command::new("gnome-extensions")
        .args(argumentos)
        .output()
        .ok()?;
    saida
        .status
        .success()
        .then(|| String::from_utf8_lossy(&saida.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEDORA: &str = "appindicatorsupport@rgcjonas.gmail.com\ndash-to-dock@micxgx.gmail.com\n";

    #[test]
    fn instalada_e_desligada_oferece_ligar() {
        assert_eq!(
            situacao_de("dash-to-dock@micxgx.gmail.com\n", FEDORA),
            DESLIGADA
        );
    }

    #[test]
    fn ligada_o_cartao_some_inclusive_a_do_ubuntu() {
        assert_eq!(situacao_de(FEDORA, FEDORA), NAO_SE_APLICA);
        let ubuntu = "ubuntu-appindicators@ubuntu.com\n";
        assert_eq!(situacao_de(ubuntu, ubuntu), NAO_SE_APLICA);
    }

    #[test]
    fn sem_a_extensao_diz_o_pacote() {
        assert_eq!(situacao_de("", "dash-to-dock@micxgx.gmail.com\n"), AUSENTE);
    }
}
