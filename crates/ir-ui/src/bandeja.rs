//! A interface na bandeja do sistema.
//!
//! No Windows a janela mora ao lado do relógio: clicar no ícone a abre, minimizar ou fechar a
//! esconde ali, e só "Sair" do menu do ícone encerra o programa. É o lugar natural de um programa
//! que fica ligado o tempo todo — o serviço trabalha sem janela nenhuma, e a interface só aparece
//! quando alguém quer olhar ou mudar algo.
//!
//! Fechar a interface nunca afeta a sessão: teclado, mouse e arquivos são do serviço
//! ([02, §1](../../../docs/02-arquitetura.md)). Esconder em vez de fechar é só para o ícone continuar
//! lá, onde o usuário o procura.
//!
//! Fora do Windows não há bandeja: o GNOME não tem uma por padrão, e a janela funciona como antes.

#[cfg(windows)]
mod instancia;

use crate::gerado::Janela;

/// Como a janela começa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inicio {
    /// Aberta na tela: o usuário acabou de pedir o programa.
    Visivel,
    /// Só o ícone: o programa subiu junto com o login (`--bandeja`).
    NaBandeja,
}

/// Se esta abertura deve seguir, ou só avisar a interface que já está aberta nesta sessão.
///
/// Chamada antes de qualquer janela existir. Devolve a marca que a interface guarda enquanto
/// viver; `None` quando outra já estava aberta — e ela foi avisada para aparecer.
#[must_use]
pub fn abrir_ou_avisar() -> Option<Marca> {
    #[cfg(windows)]
    {
        match instancia::Instancia::tomar() {
            instancia::Abertura::Primeira(marca) => Some(Marca(Some(marca))),
            instancia::Abertura::Repetida => None,
            instancia::Abertura::SemMarca => Some(Marca(None)),
        }
    }
    #[cfg(not(windows))]
    {
        Some(Marca(()))
    }
}

/// Uma marca vazia: para a demonstração, que não disputa a sessão com a interface de verdade.
#[must_use]
pub fn sem_marca() -> Marca {
    #[cfg(windows)]
    {
        Marca(None)
    }
    #[cfg(not(windows))]
    {
        Marca(())
    }
}

/// A marca de que esta é a interface da sessão.
#[derive(Debug)]
pub struct Marca(
    #[cfg(windows)] Option<instancia::Instancia>,
    #[cfg(not(windows))] (),
);

/// O ícone na bandeja, e a batida que o atende. Vive enquanto a interface viver.
#[cfg(windows)]
pub(crate) struct Bandeja {
    _icone: tray_icon::TrayIcon,
    _batida: slint::Timer,
}

/// Fora do Windows não há bandeja.
#[cfg(not(windows))]
pub(crate) struct Bandeja;

/// Roda a janela até o usuário pedir para sair.
///
/// # Errors
///
/// Repassa a falha do Slint ao mostrar a janela ou rodar o laço de eventos.
pub(crate) fn rodar(
    janela: &Janela,
    inicio: Inicio,
    marca: Marca,
) -> Result<(), slint::PlatformError> {
    #[cfg(windows)]
    if let Some(bandeja) = Bandeja::instalar(janela, marca) {
        if inicio == Inicio::Visivel {
            slint::ComponentHandle::show(janela)?;
        }
        // Com a janela escondida o laço continua: quem encerra é o "Sair" do menu do ícone.
        let resultado = slint::run_event_loop_until_quit();
        drop(bandeja);
        return resultado;
    }
    #[cfg(not(windows))]
    let _ = (Bandeja, marca);
    // Sem bandeja, começar escondido deixaria o programa rodando sem nada na tela.
    let _ = inicio;
    slint::ComponentHandle::run(janela)
}

#[cfg(windows)]
impl Bandeja {
    /// Põe o ícone na bandeja e começa a atendê-lo. `None` quando o sistema recusa o ícone — aí a
    /// janela segue sem bandeja, e fechar volta a encerrar o programa, como antes.
    fn instalar(janela: &Janela, marca: Marca) -> Option<Self> {
        use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};

        let abrir = MenuItem::new("Abrir o InputRemote", true, None);
        // Pausar sem abrir a janela: é o que se quer quando o outro computador vai ser usado por
        // outra pessoa, ou para uma apresentação.
        let pausar = MenuItem::new("Pausar o compartilhamento", true, None);
        let sair = MenuItem::new("Sair", true, None);
        let menu = Menu::new();
        menu.append(&abrir).ok()?;
        menu.append(&pausar).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&sair).ok()?;

        // O ícone 1 é o embutido no executável pelo `build.rs`: o mesmo do Explorer e do menu.
        let imagem = tray_icon::Icon::from_resource(1, None).ok()?;
        let icone = tray_icon::TrayIconBuilder::new()
            .with_icon(imagem)
            .with_tooltip("InputRemote")
            .with_menu(Box::new(menu))
            // Clique esquerdo abre a janela; o menu fica no direito, como no resto do Windows.
            .with_menu_on_left_click(false)
            .build()
            .ok()?;

        let alvo = slint::ComponentHandle::as_weak(janela);
        let itens = Itens {
            abrir: abrir.id().clone(),
            pausar,
            sair: sair.id().clone(),
        };
        let dica = icone.clone();
        let mut ultima = String::new();
        let batida = slint::Timer::default();
        batida.start(slint::TimerMode::Repeated, BATIDA, move || {
            let Some(janela) = alvo.upgrade() else { return };
            atender(&janela, &marca, &itens);
            atualizar_dica(&janela, &dica, &itens.pausar, &mut ultima);
        });
        Some(Self {
            _icone: icone,
            _batida: batida,
        })
    }
}

/// Os itens do menu que a batida atende.
#[cfg(windows)]
struct Itens {
    abrir: tray_icon::menu::MenuId,
    pausar: tray_icon::menu::MenuItem,
    sair: tray_icon::menu::MenuId,
}

/// De quanto em quanto tempo a bandeja é atendida. Um clique respondido em até 100 ms parece
/// imediato.
#[cfg(windows)]
const BATIDA: std::time::Duration = std::time::Duration::from_millis(100);

/// A dica do ícone acompanha o estado — passar o mouse sobre ele responde "está funcionando?" sem
/// abrir a janela. Antes ela dizia "InputRemote" para sempre.
#[cfg(windows)]
fn atualizar_dica(
    janela: &Janela,
    icone: &tray_icon::TrayIcon,
    pausar: &tray_icon::menu::MenuItem,
    ultima: &mut String,
) {
    use slint::ComponentHandle;
    let estado = janela.global::<crate::gerado::Dados>().get_estado();
    let dica = crate::ponte::dica_da_bandeja(&estado.enlace, &estado.resumo);
    if dica != *ultima {
        let _ = icone.set_tooltip(Some(&dica));
        pausar.set_text(if estado.pausado {
            "Retomar o compartilhamento"
        } else {
            "Pausar o compartilhamento"
        });
        pausar.set_enabled(estado.pausado || estado.conectado);
        *ultima = dica;
    }
}

/// Uma batida: cliques no ícone, escolhas do menu, a janela minimizada, e a outra abertura.
#[cfg(windows)]
fn atender(janela: &Janela, marca: &Marca, itens: &Itens) {
    use slint::ComponentHandle;
    use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

    let mut mostrar = marca
        .0
        .as_ref()
        .is_some_and(instancia::Instancia::pediram_para_mostrar);
    while let Ok(evento) = TrayIconEvent::receiver().try_recv() {
        mostrar |= matches!(
            evento,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
        );
    }
    while let Ok(escolha) = tray_icon::menu::MenuEvent::receiver().try_recv() {
        if escolha.id == itens.sair {
            let _ = slint::quit_event_loop();
            return;
        }
        if escolha.id == *itens.pausar.id() {
            let acoes = janela.global::<crate::gerado::Acoes>();
            if janela.global::<crate::gerado::Dados>().get_estado().pausado {
                acoes.invoke_retomar();
            } else {
                acoes.invoke_encerrar();
            }
        }
        mostrar |= escolha.id == itens.abrir;
    }
    let janela = janela.window();
    if mostrar {
        janela.set_minimized(false);
        let _ = janela.show();
    } else if janela.is_visible() && janela.is_minimized() {
        // Minimizar vai para a bandeja: sai da barra de tarefas e fica só o ícone.
        janela.set_minimized(false);
        let _ = janela.hide();
    }
}
