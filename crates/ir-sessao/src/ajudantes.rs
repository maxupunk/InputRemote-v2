//! Quantos ajudantes de clipboard estão ligados ao serviço agora.
//!
//! Sem o ajudante, copiar e colar simplesmente não atravessa, e nada diz por quê — foi assim duas
//! vezes: uma atualização encerrava o ajudante, e só um novo login o trazia de volta. Contar quem
//! está ligado é o que deixa o serviço relançá-lo ([`crate::zelar_pelo_clipboard`]) e o diagnóstico dizer a
//! verdade.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A contagem, compartilhada entre as conexões de controle e quem pergunta.
#[derive(Debug, Clone, Default)]
pub struct Ajudantes(Arc<AtomicUsize>);

impl Ajudantes {
    /// Quantos estão ligados.
    pub fn ligados(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }

    /// Um ajudante se apresentou. Ele conta até a [`Presenca`] ser solta — quando a conexão fecha.
    pub fn entrou(&self) -> Presenca {
        self.0.fetch_add(1, Ordering::Relaxed);
        Presenca(Arc::clone(&self.0))
    }
}

/// Um ajudante ligado. Soltá-la é a conexão dele ter fechado, por qualquer caminho.
#[derive(Debug)]
pub struct Presenca(Arc<AtomicUsize>);

impl Drop for Presenca {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conta_enquanto_a_conexao_vive() {
        let ajudantes = Ajudantes::default();
        assert_eq!(ajudantes.ligados(), 0);
        let um = ajudantes.entrou();
        let dois = ajudantes.clone().entrou();
        assert_eq!(ajudantes.ligados(), 2);
        drop(um);
        assert_eq!(ajudantes.ligados(), 1);
        drop(dois);
        assert_eq!(ajudantes.ligados(), 0);
    }
}
