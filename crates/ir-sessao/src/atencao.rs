//! Ctrl+Alt+Del gerado por software: a Sequência de Atenção Segura.
//!
//! `SendInput` não gera Ctrl+Alt+Del — por projeto do Windows. O caminho é `SendSAS(FALSE)`, de
//! `sas.dll`, chamado **pelo serviço**, e ele só funciona com a política
//! `SoftwareSASGeneration` ligando os serviços ([05, §4.3](../../../docs/05-windows.md)).
//!
//! A política é da máquina, e o produto só a toca quando um administrador liga a digitação do par
//! na tela de bloqueio — é o mesmo consentimento, com a mesma consequência. Desligar a permissão
//! devolve o valor que estava lá antes, se foi o produto quem o mudou.
//!
//! O `reg.exe` roda com prazo ([`crate::ferramenta`]) e fora do laço do serviço ([`Aplicador`]): o
//! laço bate a cada 5 ms, e um registro que trava não pode levar o teclado junto.

#![allow(unsafe_code)]

use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use tracing::{info, warn};

use ir_processo::ferramenta::{numeros_hex, rodar_com_prazo};

/// Onde mora a política.
const CHAVE: &str = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
/// O nome do valor.
const VALOR: &str = "SoftwareSASGeneration";
/// Quanto o `reg.exe` pode demorar. Ele responde em milissegundos; cinco segundos é folga para uma
/// máquina ocupada, e não uma espera sem fim.
const PRAZO_DO_REG: Duration = Duration::from_secs(5);

/// Quem pode gerar Ctrl+Alt+Del por software, pela política do sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoliticaDeAtencao {
    /// Ninguém (o padrão do Windows).
    Ninguem,
    /// Os serviços — o que o produto precisa.
    Servicos,
    /// Os aplicativos de acessibilidade.
    Acessibilidade,
    /// Os dois.
    Ambos,
}

impl PoliticaDeAtencao {
    /// Se os serviços podem gerar a sequência.
    #[must_use]
    pub const fn permite_servicos(self) -> bool {
        matches!(self, Self::Servicos | Self::Ambos)
    }

    /// A política pelo número que o registro guarda.
    #[must_use]
    pub const fn do_numero(numero: u32) -> Self {
        match numero {
            1 => Self::Servicos,
            2 => Self::Acessibilidade,
            3 => Self::Ambos,
            _ => Self::Ninguem,
        }
    }

    /// O número que o registro guarda.
    #[must_use]
    pub const fn numero(self) -> u32 {
        match self {
            Self::Ninguem => 0,
            Self::Servicos => 1,
            Self::Acessibilidade => 2,
            Self::Ambos => 3,
        }
    }

    /// A política que permite os serviços sem tirar o que já estava permitido.
    #[must_use]
    pub const fn com_servicos(self) -> Self {
        match self {
            Self::Ninguem | Self::Servicos => Self::Servicos,
            Self::Acessibilidade | Self::Ambos => Self::Ambos,
        }
    }
}

/// Lê a política, pelo `reg.exe` — o valor ausente é "ninguém".
#[must_use]
pub fn politica() -> PoliticaDeAtencao {
    // Sem o valor, o `reg.exe` sai com falha: é o "ninguém" do Windows.
    let saida = rodar_com_prazo("reg", &["query", CHAVE, "/v", VALOR], PRAZO_DO_REG);
    PoliticaDeAtencao::do_numero(saida.ok().and_then(|texto| ler_dword(&texto)).unwrap_or(0))
}

/// Grava a política.
///
/// # Errors
///
/// Se o `reg.exe` recusar — sem privilégio de administrador, por exemplo.
pub fn gravar_politica(politica: PoliticaDeAtencao) -> Result<()> {
    let numero = politica.numero().to_string();
    let argumentos = [
        "add",
        CHAVE,
        "/v",
        VALOR,
        "/t",
        "REG_DWORD",
        "/d",
        &numero,
        "/f",
    ];
    rodar_com_prazo("reg", &argumentos, PRAZO_DO_REG).map(|_| ())
}

/// O número de uma linha `SoftwareSASGeneration    REG_DWORD    0x1` do `reg query`.
///
/// Lê o hexadecimal, e não o texto em volta: o `reg.exe` muda de idioma com o sistema.
fn ler_dword(saida: &str) -> Option<u32> {
    numeros_hex(saida.lines().find(|linha| linha.contains(VALOR))?).next()
}

/// Quem liga e devolve a política fora do laço do serviço: uma thread, um pedido de cada vez, na
/// ordem em que chegaram.
///
/// Em ordem, e com o valor anterior guardado aqui dentro: permitir e recusar em seguida, com as duas
/// aplicações correndo soltas, podia fazer a recusa não achar nada a devolver — a permissão ainda
/// não tinha anotado o que havia antes — e a política ficava ligada com a permissão desligada.
#[derive(Debug)]
pub struct Aplicador {
    fila: mpsc::Sender<bool>,
}

impl Aplicador {
    /// Sobe a thread. `anterior` é o valor que a configuração guardou; `anotar` recebe o novo sempre
    /// que ele muda, para a configuração guardá-lo também.
    pub fn novo(anterior: Option<u32>, anotar: impl Fn(Option<u32>) + Send + 'static) -> Self {
        let (fila, pedidos) = mpsc::channel::<bool>();
        let criada = std::thread::Builder::new()
            .name("politica-de-atencao".to_owned())
            .spawn(move || {
                let mut anterior = anterior;
                while let Ok(ligar) = pedidos.recv() {
                    let antes = anterior;
                    aplicar(ligar, &mut anterior);
                    if anterior != antes {
                        anotar(anterior);
                    }
                }
            });
        if let Err(erro) = criada {
            warn!(%erro, "a thread da política de Ctrl+Alt+Del não subiu");
        }
        Self { fila }
    }

    /// Pede a política ligada para o serviço (`true`) ou devolvida ao que era (`false`). Não espera.
    pub fn pedir(&self, ligar: bool) {
        let _ = self.fila.send(ligar);
    }
}

/// Liga a política para o serviço, anotando o que havia, ou devolve o anotado.
fn aplicar(ligar: bool, anterior: &mut Option<u32>) {
    if ligar {
        let atual = politica();
        if atual.permite_servicos() {
            return; // já permitida, e não pelo produto: não há o que devolver depois
        }
        match gravar_politica(atual.com_servicos()) {
            Ok(()) => {
                info!(?atual, "política de Ctrl+Alt+Del ligada para o serviço");
                *anterior = Some(atual.numero());
            }
            Err(erro) => warn!(%erro, "não foi possível ligar a política de Ctrl+Alt+Del"),
        }
    } else if let Some(valor) = anterior.take()
        && let Err(erro) = gravar_politica(PoliticaDeAtencao::do_numero(valor))
    {
        warn!(%erro, "não foi possível devolver a política de Ctrl+Alt+Del");
    }
}

/// Gera Ctrl+Alt+Del na sessão de console.
///
/// Só funciona chamado por um serviço como `LocalSystem`, com a política permitindo os serviços.
/// Sem a política, o Windows simplesmente não faz nada — por isso quem chama confere antes.
pub fn enviar_sas() {
    // SAFETY: a função não recebe ponteiro e não tem pré-condição além do contexto, conferido por
    // quem chama; no contexto errado ela não faz nada.
    unsafe { windows::Win32::Security::Authentication::Identity::SendSAS(false) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leitura_vale_em_qualquer_idioma() {
        let saida = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System\r\n    SoftwareSASGeneration    REG_DWORD    0x3\r\n";
        assert_eq!(ler_dword(saida), Some(3));
        assert_eq!(ler_dword("nada aqui"), None);
    }

    #[test]
    fn permitir_os_servicos_nao_tira_a_acessibilidade() {
        assert_eq!(
            PoliticaDeAtencao::Acessibilidade.com_servicos(),
            PoliticaDeAtencao::Ambos
        );
        assert_eq!(
            PoliticaDeAtencao::Ninguem.com_servicos(),
            PoliticaDeAtencao::Servicos
        );
        assert!(PoliticaDeAtencao::Ambos.permite_servicos());
        assert!(!PoliticaDeAtencao::Acessibilidade.permite_servicos());
    }

    #[test]
    fn a_politica_desta_maquina_e_legivel() {
        // Sem administrador não se grava, mas ler sempre se lê.
        let _ = politica();
    }
}
