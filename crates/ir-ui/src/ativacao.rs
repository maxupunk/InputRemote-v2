//! Ligar o serviço e dar acesso a este usuário, pedindo a senha de administrador ao sistema.
//!
//! No Linux, o serviço sobe com o pacote, mas o acesso a ele é de quem está no grupo `inputremote`,
//! e isso é decisão de administrador. Mandar a pessoa ao terminal com um `sudo usermod` era a
//! primeira experiência com o produto. Agora a janela oferece um botão, e o **sistema** pede a senha:
//! `pkexec` mostra o diálogo do ambiente gráfico com a explicação da ação `io.github.inputremote.ativar`
//! e só então roda o ajudante instalado pelo pacote, que faz duas coisas fixas — liga o serviço e põe
//! no grupo quem pediu (`empacotar/linux/ativar`).
//!
//! A janela nunca vê a senha nem roda nada como root. Ela só pede, espera o resultado sem travar, e
//! diz o que aconteceu. Quando dá certo não há o que dizer: a ligação com o serviço, que a janela
//! tenta de novo sozinha, acontece e a tela inicial aparece.

use std::cell::RefCell;
use std::process::{Child, Command};
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode};

use crate::gerado::{Acoes, Dados, Janela};
use crate::servico::Desconexao;

/// O ajudante que o pacote instala. É o caminho que a política do polkit autoriza, e nenhum outro.
const AJUDANTE: &str = "/usr/libexec/inputremote/ativar";

/// Quem pede a senha e roda o ajudante como root.
const PKEXEC: &str = "/usr/bin/pkexec";

/// De quanto em quanto tempo o fim da ativação é conferido. Quem acabou de digitar a senha não
/// percebe um quarto de segundo.
const BATIDA: Duration = Duration::from_millis(250);

/// O que a janela diz e oferece para uma desconexão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Orientacao {
    /// O que aconteceu.
    pub frase: &'static str,
    /// O que fazer, e o que vai mudar.
    pub o_que_fazer: &'static str,
    /// O texto do botão que resolve; vazio quando não há como resolver daqui.
    pub botao: &'static str,
}

/// Se esta máquina sabe pedir a senha de administrador por um diálogo: Linux, com o polkit e o
/// ajudante do pacote instalados.
#[must_use]
pub fn disponivel() -> bool {
    // No Windows o `sc.exe` sempre existe, e quem pede a confirmação é o próprio Windows (UAC).
    cfg!(windows)
        || (cfg!(target_os = "linux")
            && std::path::Path::new(PKEXEC).exists()
            && std::path::Path::new(AJUDANTE).exists())
}

/// O comando que pede a autorização e liga o serviço.
///
/// No Linux, o `pkexec` com o ajudante do pacote. No Windows, o `sc start` elevado pelo UAC — antes
/// a janela mandava a pessoa ao console de serviços. Um diálogo recusado sai com 126, como o do
/// `pkexec`, para as frases de [`resultado`] valerem nos dois.
fn comando_de_ativacao() -> Command {
    if cfg!(windows) {
        let mut comando = Command::new("powershell");
        comando.args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "try { $p = Start-Process -FilePath sc.exe -ArgumentList 'start','InputRemote' \
             -Verb RunAs -WindowStyle Hidden -Wait -PassThru; if ($p.ExitCode -in 0,1056) \
             { exit 0 } else { exit $p.ExitCode } } catch { exit 126 }",
        ]);
        comando
    } else {
        let mut comando = Command::new(PKEXEC);
        comando.arg(AJUDANTE);
        comando
    }
}

/// O que dizer e oferecer. Com a ativação disponível, a saída é um clique; sem ela, a instrução
/// da plataforma ([`Desconexao::o_que_fazer`]).
#[must_use]
pub const fn orientacao(motivo: Desconexao, ativavel: bool) -> Orientacao {
    match (motivo, ativavel) {
        (Desconexao::ServicoParado, true) if cfg!(windows) => Orientacao {
            frase: "O serviço do InputRemote está parado.",
            o_que_fazer: "Iniciar liga de novo o serviço que leva teclado e mouse de um computador \
                          ao outro. O Windows pede a senha de administrador, ou a confirmação.",
            botao: "Iniciar o serviço",
        },
        (Desconexao::ServicoParado, true) => Orientacao {
            frase: "O InputRemote ainda não está ativado neste computador.",
            o_que_fazer: "Ativar liga o serviço que leva teclado e mouse de um computador ao \
                          outro, e ele passa a subir sozinho com o computador. O sistema pede a \
                          senha de administrador uma única vez.",
            botao: "Ativar o InputRemote",
        },
        (Desconexao::SemPermissao, true) => Orientacao {
            frase: "Falta liberar o InputRemote para este usuário.",
            o_que_fazer: "O serviço está ligado, mas só atende quem foi autorizado. O sistema pede \
                          a senha de administrador uma única vez, e vale na hora, sem reiniciar.",
            botao: "Liberar o acesso",
        },
        (motivo, false) => Orientacao {
            frase: motivo.frase(),
            o_que_fazer: motivo.o_que_fazer(),
            botao: "",
        },
    }
}

/// Como terminou uma ativação, pelo código de saída do `pkexec` — vazio quando deu certo.
///
/// 126 e 127 são do próprio `pkexec`: o diálogo foi fechado, ou a senha não autorizou. O resto vem
/// do ajudante.
#[must_use]
pub fn resultado(codigo: Option<i32>) -> String {
    match codigo {
        Some(0) => String::new(),
        Some(126) => "A autorização foi cancelada, e nada mudou. Quando quiser, é só tentar de \
                      novo."
            .to_owned(),
        Some(127) => "O sistema não autorizou. É preciso a senha de um administrador deste \
                      computador."
            .to_owned(),
        Some(outro) => format!(
            "A ativação não terminou (código {outro}). Tente de novo; se continuar, o registro do \
             sistema tem o motivo."
        ),
        None => "A ativação foi interrompida antes de terminar. Tente de novo.".to_owned(),
    }
}

/// Liga o botão da janela ao pedido de senha. `ao_ativar` é chamado quando a ativação dá certo —
/// é a deixa para refazer a ligação com o serviço na hora. O temporizador devolvido confere o fim
/// da ativação e precisa viver enquanto a janela viver.
#[must_use]
pub fn ligar(janela: &Janela, ao_ativar: impl Fn() + 'static) -> Timer {
    let em_curso: Rc<RefCell<Option<Child>>> = Rc::default();

    let alvo = janela.as_weak();
    let pedido = Rc::clone(&em_curso);
    janela.global::<Acoes>().on_ativar(move || {
        let Some(janela) = alvo.upgrade() else { return };
        if pedido.borrow().is_some() {
            return; // o diálogo já está aberto
        }
        let dados = janela.global::<Dados>();
        match comando_de_ativacao().spawn() {
            Ok(filho) => {
                *pedido.borrow_mut() = Some(filho);
                dados.set_ativando(true);
                dados.set_ativacao_resultado("".into());
            }
            Err(erro) => dados.set_ativacao_resultado(
                format!("Não foi possível pedir a autorização ao sistema: {erro}.").into(),
            ),
        }
    });

    let alvo = janela.as_weak();
    let batida = Timer::default();
    batida.start(TimerMode::Repeated, BATIDA, move || {
        let fim = em_curso
            .borrow_mut()
            .as_mut()
            .and_then(|filho| filho.try_wait().ok().flatten());
        let Some(fim) = fim else { return };
        em_curso.borrow_mut().take();
        if let Some(janela) = alvo.upgrade() {
            let dados = janela.global::<Dados>();
            dados.set_ativando(false);
            dados.set_ativacao_resultado(resultado(fim.code()).into());
        }
        if fim.success() {
            ao_ativar();
        }
    });
    batida
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODOS: [Desconexao; 2] = [Desconexao::ServicoParado, Desconexao::SemPermissao];

    #[test]
    fn com_a_ativacao_o_que_fazer_e_um_clique_e_nao_um_comando() {
        for motivo in TODOS {
            let orientacao = orientacao(motivo, true);
            assert!(!orientacao.botao.is_empty(), "{motivo:?}");
            for comando in ["sudo", "systemctl", "usermod", "terminal"] {
                assert!(
                    !orientacao.o_que_fazer.contains(comando),
                    "{motivo:?} ainda manda rodar `{comando}`"
                );
            }
            // A senha não pode ser surpresa: o texto avisa antes do diálogo aparecer.
            assert!(
                orientacao.o_que_fazer.contains("senha de administrador"),
                "{motivo:?}"
            );
        }
    }

    #[test]
    fn sem_a_ativacao_a_instrucao_da_plataforma_continua_e_nao_ha_botao() {
        for motivo in TODOS {
            let orientacao = orientacao(motivo, false);
            assert!(orientacao.botao.is_empty());
            assert_eq!(orientacao.o_que_fazer, motivo.o_que_fazer());
        }
    }

    #[test]
    fn todo_fim_que_nao_deu_certo_tem_frase_e_o_que_deu_nao_tem() {
        assert!(resultado(Some(0)).is_empty());
        for codigo in [Some(126), Some(127), Some(1), Some(64), None] {
            assert!(resultado(codigo).len() > 20, "{codigo:?}");
        }
        assert!(resultado(Some(126)).contains("nada mudou"));
    }

    #[test]
    fn no_windows_iniciar_o_servico_e_um_clique() {
        if cfg!(windows) {
            assert!(disponivel());
            let orientacao = orientacao(Desconexao::ServicoParado, true);
            assert_eq!(orientacao.botao, "Iniciar o serviço");
            assert!(
                !orientacao.o_que_fazer.contains("Serviços"),
                "{}",
                orientacao.o_que_fazer
            );
        }
    }
}
