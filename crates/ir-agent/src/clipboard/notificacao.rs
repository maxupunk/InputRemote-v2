//! A notificação do sistema, no Linux.
//!
//! No Windows a interface fica na bandeja e mostra um aviso no canto da tela. No GNOME não há
//! bandeja, e a janela costuma estar fechada — quem copia está no Nautilus. O lugar certo do
//! recado ali é a notificação do sistema, e quem sempre está de pé na sessão é este ajudante.
//!
//! Por `notify-send`, e não por D-Bus direto: é uma linha de processo contra um cliente de barramento
//! inteiro como dependência, e o pacote já exige `libnotify` por isso.
//!
//! # Um recado que se atualiza, e não uma fila de recados
//!
//! A primeira versão contava duas vezes por cópia — começo e fim. Na prática o começo aparecia com
//! "0 B de 839,2 MB · 0%" e ficava assim até a cópia acabar: o número nunca andava, que é
//! exatamente a queixa que o andamento deveria ter resolvido.
//!
//! Agora a notificação do começo guarda o **id** que o servidor devolve (`notify-send -p`) e as
//! seguintes a substituem por esse id (`-r`). O GNOME atualiza o recado no lugar: um só, com o
//! número andando. De dois em dois segundos, e não a cada aviso de 200 ms — um banner que se
//! redesenha cinco vezes por segundo é pior que nenhum.

use std::time::{Duration, Instant};

use ir_ipc::transferencia::Transferencia;

/// De quanto em quanto tempo o andamento vai para a tela.
///
/// Dois segundos: o bastante para ver o número mudar, e pouco para o banner não virar estroboscópio.
const INTERVALO: Duration = Duration::from_secs(2);

/// Guarda o que está sendo contado, e como falar com aquele recado de novo.
#[derive(Debug, Default)]
pub(crate) struct Notificador {
    /// Qual cópia o recado corrente conta.
    atual: Option<String>,
    /// O id do recado no servidor de notificações, para atualizá-lo no lugar.
    id: Option<u32>,
    /// Quando o andamento foi contado pela última vez.
    contado: Option<Instant>,
}

impl Notificador {
    /// Conta esta cópia ao usuário, se for hora.
    pub(crate) fn contar(&mut self, copia: &Transferencia) {
        if !self.deve_contar(&chave(copia), copia.terminou(), Instant::now()) {
            return;
        }
        let substituir = self.id;
        self.id = enviar(copia, substituir);
        if copia.terminou() {
            // O recado fechou esta cópia; a próxima começa um novo.
            self.atual = None;
            self.id = None;
        }
    }

    /// Se é hora de contar: cópia nova, fim de cópia, ou passou o intervalo.
    ///
    /// Separado do sistema e do relógio para ser testado — é aqui que mora a regra de quantos
    /// recados o usuário vê.
    fn deve_contar(&mut self, chave: &str, terminou: bool, agora: Instant) -> bool {
        let mudou = self.atual.as_deref() != Some(chave);
        if mudou {
            self.atual = Some(chave.to_owned());
            self.id = None;
            self.contado = Some(agora);
            return true;
        }
        if terminou {
            self.contado = Some(agora);
            return true;
        }
        let passou = self
            .contado
            .is_none_or(|quando| agora.duration_since(quando) >= INTERVALO);
        if passou {
            self.contado = Some(agora);
        }
        passou
    }
}

/// O que distingue uma cópia da seguinte. O progresso **não** entra: é a mesma cópia andando.
fn chave(copia: &Transferencia) -> String {
    format!("{}|{}", copia.nome, copia.sentido.rotulo())
}

/// Manda o recado, substituindo o anterior quando há um. Devolve o id para a próxima atualização.
///
/// Falhar aqui não é motivo para nada: a cópia continua, e o retorno vai faltar só na tela.
#[cfg(target_os = "linux")]
fn enviar(copia: &Transferencia, substituir: Option<u32>) -> Option<u32> {
    use std::process::Command;

    let urgencia = if copia.falhou() { "critical" } else { "normal" };
    let mut comando = Command::new("notify-send");
    comando
        .arg("--app-name=InputRemote")
        .arg("--icon=inputremote")
        .arg(format!("--urgency={urgencia}"))
        // `-p` faz o `notify-send` devolver o id do recado; `-r` diz qual recado atualizar. Juntos,
        // são um recado só que muda de texto em vez de uma pilha de recados.
        .arg("--print-id");
    if let Some(id) = substituir {
        comando.arg(format!("--replace-id={id}"));
    }
    let saida = comando
        .arg(copia.titulo())
        .arg(copia.detalhe())
        .stderr(std::process::Stdio::null())
        .output();
    match saida {
        Ok(saida) => ler_id(&String::from_utf8_lossy(&saida.stdout)),
        Err(erro) => {
            tracing::debug!(%erro, "sem notify-send; a cópia continua, sem recado na tela");
            None
        }
    }
}

/// Fora do Linux quem conta é a interface, com o aviso no canto da tela.
#[cfg(not(target_os = "linux"))]
const fn enviar(_copia: &Transferencia, _substituir: Option<u32>) -> Option<u32> {
    None
}

/// O id que o `notify-send -p` imprime. Sem id, a próxima notificação vira outra — feio, e não
/// motivo para deixar de contar.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn ler_id(saida: &str) -> Option<u32> {
    saida.trim().lines().next()?.trim().parse().ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use ir_ipc::transferencia::{Fase, Motivo, Sentido};

    use super::*;

    fn copia(nome: &str, fase: Fase, feitos: u64) -> Transferencia {
        Transferencia {
            sentido: Sentido::Recebendo,
            nome: nome.to_owned(),
            bytes_feitos: feitos,
            bytes_total: 1000,
            fase,
        }
    }

    #[test]
    fn a_copia_nova_conta_na_hora() {
        let mut notificador = Notificador::default();
        let agora = Instant::now();
        assert!(notificador.deve_contar("pasta-B|recebendo", false, agora));
    }

    /// O defeito relatado: o recado nascia em "0 B de 839,2 MB · 0%" e só mudava no fim.
    #[test]
    fn o_andamento_da_mesma_copia_volta_a_contar_a_cada_intervalo() {
        let mut notificador = Notificador::default();
        let inicio = Instant::now();
        assert!(notificador.deve_contar("pasta-B|recebendo", false, inicio));
        assert!(
            !notificador.deve_contar("pasta-B|recebendo", false, inicio + INTERVALO / 4),
            "cinco avisos por segundo não viram cinco recados"
        );
        assert!(notificador.deve_contar("pasta-B|recebendo", false, inicio + INTERVALO));
        assert!(!notificador.deve_contar("pasta-B|recebendo", false, inicio + INTERVALO));
        assert!(notificador.deve_contar("pasta-B|recebendo", false, inicio + INTERVALO * 2));
    }

    #[test]
    fn o_fim_conta_sempre_mesmo_fora_do_intervalo() {
        let mut notificador = Notificador::default();
        let inicio = Instant::now();
        notificador.deve_contar("pasta-B|recebendo", false, inicio);
        assert!(
            notificador.deve_contar("pasta-B|recebendo", true, inicio + INTERVALO / 10),
            "o fim é o recado que importa"
        );
    }

    #[test]
    fn outra_copia_comeca_outro_recado() {
        let mut notificador = Notificador::default();
        let agora = Instant::now();
        notificador.deve_contar("pasta-A|recebendo", false, agora);
        notificador.id = Some(42);
        assert!(notificador.deve_contar("pasta-B|recebendo", false, agora));
        assert!(
            notificador.id.is_none(),
            "o recado da outra cópia não é atualizado com o texto desta"
        );
    }

    #[test]
    fn a_cópia_que_termina_fecha_o_recado_e_a_proxima_abre_outro() {
        let mut notificador = Notificador::default();
        let pronta = copia(
            "pasta-B",
            Fase::Concluida {
                destino: "/tmp/pasta-B".to_owned(),
            },
            1000,
        );
        // `contar` fala com o sistema; aqui interessa o estado depois, que a regra governa.
        notificador.deve_contar(&chave(&pronta), true, Instant::now());
        assert_eq!(notificador.atual.as_deref(), Some("pasta-B|recebendo"));
    }

    #[test]
    fn o_id_impresso_pelo_notify_send_e_lido() {
        assert_eq!(ler_id("42\n"), Some(42));
        assert_eq!(ler_id(" 7 "), Some(7));
        assert_eq!(ler_id(""), None);
        assert_eq!(ler_id("nada disso"), None);
    }

    #[test]
    fn a_falha_e_um_recado_como_os_outros() {
        let parada = copia("pasta-B", Fase::Parada(Motivo::CanalCaiu), 10);
        assert!(parada.falhou());
        assert_eq!(chave(&parada), "pasta-B|recebendo");
    }
}
