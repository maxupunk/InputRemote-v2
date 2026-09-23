//! Se a tela desta máquina está bloqueada, ou no login, pelo `logind`.
//!
//! No Linux o serviço injeta direto por `uinput`, que chega ao greeter e à tela de bloqueio — e é
//! por isso que precisa saber quando está numa delas: sem a permissão do administrador daqui, o
//! que o par digita ali é descartado ([04, §6](../../../docs/04-seguranca.md)).

use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;

/// De quanto em quanto tempo se pergunta ao `logind`.
const PERGUNTAR_A_CADA: Duration = Duration::from_secs(2);

/// Vigia a tela, e conta a `destino` cada vez que ela passa a estar protegida, ou deixa de estar.
/// Precisa de uma runtime `tokio`.
pub fn vigiar_a_tela(destino: UnboundedSender<bool>) {
    tokio::spawn(async move {
        let mut anterior = None;
        loop {
            let protegida = tokio::task::spawn_blocking(tela_protegida_agora)
                .await
                .unwrap_or(false);
            if anterior != Some(protegida) {
                anterior = Some(protegida);
                if destino.send(protegida).is_err() {
                    return;
                }
            }
            tokio::time::sleep(PERGUNTAR_A_CADA).await;
        }
    });
}

/// Pergunta ao `loginctl` pela sessão ativa do assento principal.
fn tela_protegida_agora() -> bool {
    let rodar = |argumentos: &[&str]| {
        std::process::Command::new("loginctl")
            .args(argumentos)
            .output()
            .ok()
            .filter(|saida| saida.status.success())
            .map(|saida| String::from_utf8_lossy(&saida.stdout).into_owned())
    };
    let Some(ativa) = rodar(&["show-seat", "seat0", "-p", "ActiveSession", "--value"]) else {
        return false; // sem assento: uma máquina sem tela, e não há o que proteger
    };
    let ativa = ativa.trim();
    if ativa.is_empty() {
        return true; // ninguém no assento: é a tela de login
    }
    rodar(&["show-session", ativa, "-p", "Class", "-p", "LockedHint"])
        .is_some_and(|saida| sessao_protegida(&saida))
}

/// Se as propriedades de uma sessão dizem greeter ou bloqueio.
fn sessao_protegida(propriedades: &str) -> bool {
    propriedades.lines().any(|linha| {
        let linha = linha.trim();
        linha == "Class=greeter" || linha == "LockedHint=yes"
    })
}

#[cfg(test)]
mod tests {
    use super::sessao_protegida;

    #[test]
    fn greeter_e_bloqueio_sao_telas_protegidas() {
        assert!(sessao_protegida("Class=greeter\nLockedHint=no\n"));
        assert!(sessao_protegida("Class=user\nLockedHint=yes\n"));
        assert!(!sessao_protegida("Class=user\nLockedHint=no\n"));
    }
}
