//! O relógio fino do Windows: o serviço bate a sessão a cada 5 ms, e o Windows não deixa.
//!
//! O temporizador padrão do Windows anda em passos de 15,6 ms. Medido nesta máquina, a batida de
//! 5 ms do ator saía a cada **13,2 ms**; com o relógio de 1 ms pedido, a cada 5,0 ms. Tudo o que
//! a sessão faz por tempo andava duas a três vezes mais devagar do que o projetado: a confirmação
//! pura de 20 ms, a retransmissão do piso, e o último pedaço de um movimento do mouse, que só sai
//! na batida seguinte.
//!
//! [`fino`] pede o relógio de 1 ms enquanto a guarda viver. Custa um pouco de energia com o serviço
//! de pé, que é o preço de uma ferramenta de entrada responder como uma. Nos outros sistemas o
//! relógio já é fino, e a guarda não faz nada.

/// Enquanto viver, o relógio do processo anda em passos de 1 ms.
#[derive(Debug)]
#[must_use = "o relógio volta ao padrão quando a guarda é solta"]
pub struct RelogioFino {
    #[cfg(windows)]
    pedido: bool,
}

/// Pede o relógio de 1 ms ao sistema.
pub fn fino() -> RelogioFino {
    #[cfg(windows)]
    {
        let pedido = windows::pedir();
        if !pedido {
            tracing::warn!(
                "o Windows recusou o relógio de 1 ms; as batidas do serviço ficam lentas"
            );
        }
        RelogioFino { pedido }
    }
    #[cfg(not(windows))]
    RelogioFino {}
}

#[cfg(windows)]
impl Drop for RelogioFino {
    fn drop(&mut self) {
        if self.pedido {
            windows::devolver();
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use ::windows::Win32::Media::{TIMERR_NOERROR, timeBeginPeriod, timeEndPeriod};

    /// A resolução pedida, em milissegundos.
    const PERIODO: u32 = 1;

    pub(super) fn pedir() -> bool {
        // SAFETY: recebe um inteiro e só muda a resolução do temporizador deste processo.
        unsafe { timeBeginPeriod(PERIODO) == TIMERR_NOERROR }
    }

    pub(super) fn devolver() {
        // SAFETY: desfaz o `timeBeginPeriod` do mesmo período, como a documentação pede.
        unsafe {
            let _ = timeEndPeriod(PERIODO);
        }
    }
}
