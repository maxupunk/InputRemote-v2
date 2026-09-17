//! Uma pasta temporária para os testes, sem dependência nova.
//!
//! `tempfile` resolveria isto, mas toda dependência nova exige decisão registrada em
//! [07](../../../docs/07-stack-e-dependencias.md), e trazer um crate para vinte linhas de teste
//! não se justifica. O que se precisa aqui é pouco: um nome que não colida e uma limpeza que
//! aconteça sozinha.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Distingue pastas criadas dentro do mesmo processo; o pid distingue entre processos.
static CONTADOR: AtomicU32 = AtomicU32::new(0);

/// Uma pasta que se apaga quando sai de escopo.
#[derive(Debug)]
pub(crate) struct PastaTemporaria {
    caminho: PathBuf,
}

impl PastaTemporaria {
    /// A pasta.
    pub(crate) fn caminho(&self) -> &Path {
        &self.caminho
    }
}

impl Drop for PastaTemporaria {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.caminho);
    }
}

/// Cria uma pasta temporária com um nome que não colide.
///
/// `cargo test` roda os testes em paralelo no mesmo processo, então o contador é obrigatório; e
/// `cargo test` de dois crates roda em processos diferentes, então o pid também é.
pub(crate) fn pasta_temporaria(rotulo: &str) -> PastaTemporaria {
    let n = CONTADOR.fetch_add(1, Ordering::Relaxed);
    let caminho =
        std::env::temp_dir().join(format!("ir-files-{rotulo}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&caminho);
    std::fs::create_dir_all(&caminho).expect("criar a pasta temporária do teste");
    PastaTemporaria { caminho }
}
