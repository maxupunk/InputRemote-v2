//! O arquivo que está atravessando agora, de um lado ou do outro.
//!
//! Quem envia lê e quem recebe escreve, mas a contabilidade é a mesma: de que item é, quanto já
//! passou, quanto o manifesto prometeu e o BLAKE3 do que passou. Ela morava duplicada em
//! [`crate::envio`] e [`crate::recepcao`], e é exatamente a parte que cobra a garantia de cópia —
//! um lado que conferisse o tamanho de um jeito e o outro de outro deixaria passar o que um deles
//! recusa.

use std::path::{Path, PathBuf};

/// Um arquivo aberto no meio da travessia.
#[derive(Debug)]
pub(crate) struct ArquivoEmCurso {
    item: u32,
    caminho: PathBuf,
    arquivo: tokio::fs::File,
    feitos: u64,
    declarado: u64,
    resumo: blake3::Hasher,
}

impl ArquivoEmCurso {
    /// Começa a contar o item `item`, que o manifesto declarou com `declarado` bytes.
    pub(crate) fn novo(
        item: u32,
        caminho: PathBuf,
        arquivo: tokio::fs::File,
        declarado: u64,
    ) -> Self {
        Self {
            item,
            caminho,
            arquivo,
            feitos: 0,
            declarado,
            resumo: blake3::Hasher::new(),
        }
    }

    /// De que item do manifesto é este arquivo.
    pub(crate) const fn item(&self) -> u32 {
        self.item
    }

    /// Onde ele está, para as mensagens de erro.
    pub(crate) fn caminho(&self) -> &Path {
        &self.caminho
    }

    /// O arquivo em si, para ler ou escrever.
    pub(crate) const fn arquivo(&mut self) -> &mut tokio::fs::File {
        &mut self.arquivo
    }

    /// Quantos bytes já passaram — é o deslocamento do próximo bloco.
    pub(crate) const fn feitos(&self) -> u64 {
        self.feitos
    }

    /// Quantos bytes o manifesto prometeu.
    pub(crate) const fn declarado(&self) -> u64 {
        self.declarado
    }

    /// Se mais `tamanho` bytes ainda cabem no que o manifesto declarou.
    pub(crate) fn cabe(&self, tamanho: usize) -> bool {
        u64::try_from(tamanho)
            .ok()
            .and_then(|tamanho| self.feitos.checked_add(tamanho))
            .is_some_and(|fim| fim <= self.declarado)
    }

    /// Conta um bloco que passou: no resumo e no total. Devolve o tamanho dele.
    pub(crate) fn contar(&mut self, dados: &[u8]) -> u64 {
        self.resumo.update(dados);
        let tamanho = u64::try_from(dados.len()).unwrap_or(u64::MAX);
        self.feitos = self.feitos.saturating_add(tamanho);
        tamanho
    }

    /// Se passou exatamente o que o manifesto prometeu.
    pub(crate) const fn completo(&self) -> bool {
        self.feitos == self.declarado
    }

    /// O BLAKE3 de tudo o que passou.
    pub(crate) fn resumo(&self) -> [u8; 32] {
        *self.resumo.finalize().as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teste::{PastaTemporaria, pasta_temporaria};

    async fn em_curso(declarado: u64) -> (PastaTemporaria, ArquivoEmCurso) {
        let pasta = pasta_temporaria("em-curso");
        let caminho = pasta.caminho().join("a.bin");
        let arquivo = tokio::fs::File::create(&caminho).await.unwrap();
        (pasta, ArquivoEmCurso::novo(7, caminho, arquivo, declarado))
    }

    #[tokio::test]
    async fn conta_o_que_passa_e_so_aceita_o_declarado() {
        let (_pasta, mut aberto) = em_curso(5).await;
        assert_eq!(aberto.item(), 7);
        assert!(aberto.cabe(5));
        assert!(!aberto.cabe(6), "um byte além do declarado não cabe");
        assert_eq!(aberto.contar(b"abc"), 3);
        assert_eq!(aberto.feitos(), 3);
        assert!(!aberto.completo());
        assert!(!aberto.cabe(3));
        aberto.contar(b"de");
        assert!(aberto.completo());
        assert_eq!(aberto.resumo(), *blake3::hash(b"abcde").as_bytes());
    }
}
