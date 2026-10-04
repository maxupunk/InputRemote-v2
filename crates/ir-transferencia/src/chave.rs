//! A chave de "Copiar e colar", das Preferências.
//!
//! Uma decisão só, deste computador, valendo para os dois sentidos: desligada, o que se copia aqui
//! não vai ao outro computador, e o que se copia lá não chega ao clipboard daqui. Quem grava a
//! escolha é o serviço; quem a consulta são as peças que deixam a cópia passar — o ator, a recepção
//! de arquivos ([`crate::recebendo`]) e o canal das pastas, pela [`crate::Faixa`]. Todas leem a
//! mesma chave, e nenhuma guarda cópia dela.
//!
//! A pasta compartilhada não passa por aqui: sincronizar não é copiar e colar.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Se copiar e colar está ligado neste computador. Nasce ligada.
#[derive(Debug, Clone)]
pub struct ChaveDaCopia(Arc<AtomicBool>);

impl Default for ChaveDaCopia {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}

impl ChaveDaCopia {
    /// Liga ou desliga, para todos que leem esta chave.
    pub fn ligar(&self, ligada: bool) {
        self.0.store(ligada, Ordering::Relaxed);
    }

    /// Se a cópia pode passar agora.
    #[must_use]
    pub fn ligada(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn nasce_ligada_e_quem_tem_um_clone_ve_a_mudanca() {
        let chave = ChaveDaCopia::default();
        let do_canal_das_pastas = chave.clone();
        assert!(do_canal_das_pastas.ligada());
        chave.ligar(false);
        assert!(!do_canal_das_pastas.ligada());
    }
}
