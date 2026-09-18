//! O aviso de mudança do clipboard, sem *polling*.
//!
//! `AddClipboardFormatListener` exige uma janela, porque é para ela que o `WM_CLIPBOARDUPDATE` é
//! postado. Então cria-se uma janela **sem tela** — filha de `HWND_MESSAGE`, que existe no Win32
//! exatamente para isto: receber mensagem sem nunca ser desenhada, sem aparecer na barra de
//! tarefas e sem roubar foco.
//!
//! # Por que não *polling*
//!
//! [05, §6](../../../docs/05-windows.md) proíbe, e a razão é medida, não estética:
//! `GetClipboardSequenceNumber` num laço acorda o processador algumas vezes por segundo para
//! descobrir, quase sempre, que nada mudou. Num produto que roda o dia inteiro em notebook, isso é
//! bateria gasta para nada. O sistema sabe avisar; basta pedir.
//!
//! # A thread importa
//!
//! `GetMessageW` entrega mensagens da fila da **thread que criou a janela**. Logo a janela é criada e
//! bombeada na mesma thread, e quem usa este tipo dedica uma a ele. Criar aqui e bombear ali é o
//! erro que faz o aviso nunca chegar — e ele não dá erro, só silêncio.

#![allow(unsafe_code)]

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetMessageW, HWND_MESSAGE, MSG, RegisterClassW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WNDCLASSW,
};
use windows::core::PCWSTR;

use crate::Vigia;
use crate::error::{ClipError, Result};

/// O nome da classe de janela. Terminado em zero, como o Win32 exige.
const CLASSE: &[u16] = &[
    b'I' as u16,
    b'R' as u16,
    b'C' as u16,
    b'l' as u16,
    b'i' as u16,
    b'p' as u16,
    0,
];

/// Uma janela sem tela, só para ouvir o clipboard.
#[derive(Debug)]
pub(super) struct VigiaDoWindows {
    janela: HWND,
}

impl VigiaDoWindows {
    /// Registra a classe, cria a janela e pede o aviso.
    ///
    /// # Errors
    ///
    /// [`ClipError::Indisponivel`] onde não há *window station* alcançável — o caso do serviço na
    /// sessão 0, que é justamente por que o clipboard é do agente e não dele.
    pub(super) fn nova() -> Result<Self> {
        let classe = WNDCLASSW {
            lpfnWndProc: Some(janela_padrao),
            lpszClassName: PCWSTR(CLASSE.as_ptr()),
            ..WNDCLASSW::default()
        };
        // Registrar duas vezes a mesma classe devolve zero, e é um caso normal: uma segunda
        // instância do vigia na mesma thread reaproveita a classe já registrada.
        let _ = unsafe { RegisterClassW(&raw const classe) };

        let janela = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(CLASSE.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(|_| ClipError::Indisponivel("não há window station para ouvir o clipboard"))?;

        unsafe { AddClipboardFormatListener(janela) }
            .map_err(|_| ClipError::Indisponivel("o sistema recusou o aviso de clipboard"))?;
        Ok(Self { janela })
    }
}

impl Vigia for VigiaDoWindows {
    fn proxima(&mut self) -> Result<()> {
        loop {
            let mut mensagem = MSG::default();
            // Filtrado pela nossa janela: nada mais desta thread nos interessa.
            let resultado = unsafe { GetMessageW(&raw mut mensagem, Some(self.janela), 0, 0) };
            if resultado.0 <= 0 {
                // Zero é `WM_QUIT`; negativo é erro. Nos dois casos não há mais avisos a esperar, e
                // dizer isso é melhor que repetir num laço quente.
                return Err(ClipError::Indisponivel("a fila de mensagens encerrou"));
            }
            if mensagem.message == WM_CLIPBOARDUPDATE {
                return Ok(());
            }
        }
    }
}

impl Drop for VigiaDoWindows {
    fn drop(&mut self) {
        // Na ordem inversa da criação. Deixar o ouvinte registrado para uma janela destruída faz o
        // sistema postar mensagem para um alvo que não existe.
        let _ = unsafe { RemoveClipboardFormatListener(self.janela) };
        let _ = unsafe { DestroyWindow(self.janela) };
    }
}

/// O tratador da janela: não faz nada, e é o certo.
///
/// A mensagem que interessa é lida por [`Vigia::proxima`], na fila. Tratá-la aqui exigiria um canal
/// saindo de um `extern "system"`, que é estado global disfarçado.
unsafe extern "system" fn janela_padrao(
    janela: HWND,
    mensagem: u32,
    w: WPARAM,
    l: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcW(janela, mensagem, w, l) }
}
