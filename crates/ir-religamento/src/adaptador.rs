//! O rádio desta máquina, visto pelo sistema: quem mais está conectado, e religar.
//!
//! No Linux, pelo `bluetoothctl`, que fala com o `bluetoothd`: o serviço roda como root, e o
//! `bluetoothd` fica sabendo do religamento como se a pessoa o tivesse feito pelo menu — sem um
//! rádio ligado por baixo dele que ele acha desligado. Sem `unsafe` e sem biblioteca de D-Bus,
//! que o RPM não tem como compilar (docs/logs/26).
//!
//! No Windows ainda não há religamento: o aviso ensina o caminho do sistema.

#[cfg(target_os = "linux")]
use std::time::Duration;

/// O que se pede ao rádio desta máquina.
///
/// Um trait, e não funções soltas, para o [`crate::Vigia`] ser testado sem rádio nenhum.
pub trait Adaptador: Send + Sync + 'static {
    /// Se esta máquina sabe religar o rádio sozinha.
    fn sabe_religar(&self) -> bool;

    /// Quantos aparelhos Bluetooth estão conectados a esta máquina agora.
    ///
    /// # Errors
    ///
    /// O motivo, quando não deu para perguntar ao sistema.
    fn aparelhos_conectados(&self) -> Result<usize, String>;

    /// Desliga e liga o rádio.
    ///
    /// # Errors
    ///
    /// O motivo, quando o sistema recusou.
    fn religar(&self) -> Result<(), String>;
}

/// O adaptador desta plataforma.
#[must_use]
pub fn do_sistema() -> std::sync::Arc<dyn Adaptador> {
    #[cfg(target_os = "linux")]
    {
        std::sync::Arc::new(Bluetoothctl)
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::sync::Arc::new(SemReligamento)
    }
}

/// Quantos aparelhos a saída de `bluetoothctl devices Connected` lista.
///
/// Uma linha `Device <endereço> <nome>` por aparelho; o resto (avisos coloridos de mudança, linhas
/// vazias) não conta.
#[must_use]
pub fn contar_aparelhos(saida: &str) -> usize {
    saida
        .lines()
        .filter(|linha| linha.trim_start().starts_with("Device "))
        .count()
}

/// Quanto o `bluetoothctl` pode demorar por comando.
#[cfg(target_os = "linux")]
const PRAZO_DA_FERRAMENTA: Duration = Duration::from_secs(10);

/// O intervalo entre desligar e ligar: o bastante para a controladora soltar o que a prendia.
#[cfg(target_os = "linux")]
const DESLIGADO_POR: Duration = Duration::from_secs(2);

/// O rádio pelo `bluetoothctl`.
#[cfg(target_os = "linux")]
struct Bluetoothctl;

#[cfg(target_os = "linux")]
impl Bluetoothctl {
    fn rodar(argumentos: &[&str]) -> Result<String, String> {
        ir_processo::ferramenta::rodar_com_prazo("bluetoothctl", argumentos, PRAZO_DA_FERRAMENTA)
            .map_err(|erro| format!("{erro:#}"))
    }
}

#[cfg(target_os = "linux")]
impl Adaptador for Bluetoothctl {
    fn sabe_religar(&self) -> bool {
        true
    }

    fn aparelhos_conectados(&self) -> Result<usize, String> {
        Self::rodar(&["devices", "Connected"]).map(|saida| contar_aparelhos(&saida))
    }

    fn religar(&self) -> Result<(), String> {
        Self::rodar(&["power", "off"])?;
        std::thread::sleep(DESLIGADO_POR);
        Self::rodar(&["power", "on"]).map(drop)
    }
}

/// Onde não se sabe religar o rádio: a janela ensina o caminho do sistema.
#[cfg(not(target_os = "linux"))]
struct SemReligamento;

#[cfg(not(target_os = "linux"))]
impl Adaptador for SemReligamento {
    fn sabe_religar(&self) -> bool {
        false
    }

    fn aparelhos_conectados(&self) -> Result<usize, String> {
        Err("sem suporte neste sistema".to_owned())
    }

    fn religar(&self) -> Result<(), String> {
        Err("sem suporte neste sistema".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conta_so_as_linhas_de_aparelho() {
        let saida = "\u{1b}[0;93m[CHG]\u{1b}[0m Controller AC:50:DE:47:EB:28 Powered: yes\n\
                     Device 74:13:EA:A6:5A:99 SAMSUNG-MAXUEL\n\
                     Device 11:22:33:44:55:66 Fone\n\n";
        assert_eq!(contar_aparelhos(saida), 2);
        assert_eq!(contar_aparelhos(""), 0);
    }
}
