//! Uma janela por sessão: a segunda abertura traz a primeira à frente e sai.
//!
//! Com a interface morando na bandeja, abrir o programa pelo menu Iniciar abriria uma segunda
//! janela ao lado da que está escondida — duas interfaces falando com o mesmo serviço, cada uma com
//! o seu ícone. O jeito do Windows de dizer "já existe" é um objeto nomeado: a primeira instância
//! cria um evento, a segunda o encontra, sinaliza e sai, e a primeira vê o sinal e mostra a janela.
//!
//! `Local\` põe o nome no espaço da sessão: com troca rápida de usuário, cada pessoa tem a própria
//! interface, e a de uma não é acordada pela outra.

#![allow(unsafe_code)]

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};

/// O nome do evento, em UTF-16 terminado em nulo.
fn nome() -> Vec<u16> {
    "Local\\InputRemote-interface-mostrar"
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

/// A marca de que esta é a interface desta sessão.
#[derive(Debug)]
pub(crate) struct Instancia(HANDLE);

/// O que a abertura descobriu.
#[derive(Debug)]
pub(crate) enum Abertura {
    /// É a primeira da sessão: fica com a marca.
    Primeira(Instancia),
    /// Já havia uma, que foi avisada para aparecer. Quem chama sai.
    Repetida,
    /// O sistema recusou criar o evento. Segue sem a marca: uma janela a mais é um incômodo, e
    /// nenhuma janela seria um programa que não abre.
    SemMarca,
}

impl Instancia {
    /// Toma a marca da sessão, ou avisa quem já a tem.
    pub(crate) fn tomar() -> Abertura {
        let nome = nome();
        // SAFETY: `nome` vive até o fim da chamada e termina em nulo; sem atributos de segurança
        // (os padrões do processo); reinício automático, começando não sinalizado.
        let evento = unsafe { CreateEventW(std::ptr::null(), 0, 0, nome.as_ptr()) };
        if evento.is_null() {
            return Abertura::SemMarca;
        }
        // SAFETY: lê o erro da chamada imediatamente anterior, nesta mesma thread.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: `evento` é um handle válido recém-aberto; sinalizar e fechar uma vez.
            unsafe {
                SetEvent(evento);
                CloseHandle(evento);
            }
            return Abertura::Repetida;
        }
        Abertura::Primeira(Self(evento))
    }

    /// Se outra abertura pediu para a janela aparecer desde a última pergunta.
    pub(crate) fn pediram_para_mostrar(&self) -> bool {
        // SAFETY: handle válido enquanto `self` viver; espera zero, só consulta. O evento se
        // rearma sozinho ao ser observado (reinício automático).
        unsafe { WaitForSingleObject(self.0, 0) == 0 }
    }
}

impl Drop for Instancia {
    fn drop(&mut self) {
        // SAFETY: o handle foi criado em `tomar` e só é fechado aqui.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
