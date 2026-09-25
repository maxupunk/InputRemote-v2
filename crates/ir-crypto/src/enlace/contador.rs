//! O contador que as duas pontas contam, em vez de mandar.
//!
//! Sobre um *stream* (RFCOMM, TCP) a ordem é garantida pelo meio, então quem recebe **conta** os
//! quadros — o primeiro é 1, o seguinte é 2 — e chega ao mesmo número que o
//! [`Sealer`](crate::Sealer) emitiu do outro lado ([03, §3.1](../../../docs/03-protocolo.md)). São 8
//! bytes a menos por quadro que no UDP, onde o contador viaja em claro.
//!
//! # A regra que tem de estar num lugar só
//!
//! **A contagem só avança depois de a tag conferir.** Se avançasse na tentativa, um corpo forjado
//! queimaria aquele número, e o quadro legítimo seguinte — que vem com ele — deixaria de abrir para
//! sempre: bastaria escrever lixo no socket para dessincronizar um enlace que, sem o atacante,
//! funcionaria. É a mesma disciplina que o [`Opener`](crate::Opener) aplica à janela de repetição.

/// Quantos quadros já foram abertos; o próximo contador é este mais um.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContadorImplicito {
    recebidos: u64,
}

impl ContadorImplicito {
    /// Uma contagem nova, antes do primeiro quadro.
    #[must_use]
    pub const fn novo() -> Self {
        Self { recebidos: 0 }
    }

    /// Quantos quadros já foram abertos com sucesso.
    #[must_use]
    pub const fn recebidos(&self) -> u64 {
        self.recebidos
    }

    /// Abre o próximo quadro com o contador esperado, e só avança se `abrir` der certo.
    ///
    /// `abrir` recebe o contador e decifra — com um [`Transport`](crate::Transport) inteiro ou com
    /// um [`Opener`](crate::Opener), o que o enlace tiver.
    ///
    /// # Errors
    ///
    /// Os de `abrir`, sem tocar na contagem.
    pub fn abrir<T, E>(&mut self, abrir: impl FnOnce(u64) -> Result<T, E>) -> Result<T, E> {
        let contador = self.recebidos.wrapping_add(1);
        let aberto = abrir(contador)?;
        self.recebidos = contador;
        Ok(aberto)
    }
}
