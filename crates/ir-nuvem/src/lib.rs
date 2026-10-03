//! A pasta recebida sob demanda: no Windows, pela Cloud Files API — a mesma do OneDrive —; no
//! Linux, por um sistema de arquivos em espaço de usuário ([ADR-0015](../../../docs/adr/0015-pastas-compartilhadas.md), §3).
//!
//! A pasta da réplica vira uma **raiz de sincronia**: cada arquivo da origem aparece nela como um
//! *marcador* — nome, tamanho e data certos, sem os bytes. Quando um programa lê um marcador, o
//! Windows chama este provedor ([`Pedido::Buscar`]); o ajudante busca o trecho na origem e o
//! entrega ([`entregar`]). Os ícones de nuvem e de ✓, e o menu "Sempre manter neste dispositivo /
//! Liberar espaço", são do próprio Explorer.
//!
//! Este crate não conhece a sincronia: ele embrulha a API do sistema em funções seguras, e quem
//! decide quando chamá-las é o `ir-sincronia`. No Linux, o módulo `linux` monta o cache da réplica
//! e espera o conteúdo de um arquivo quando ele é aberto. Onde nenhum dos dois serve — fora de NTFS,
//! sem `fusermount3` —, [`suportado`] diz que não, e a réplica baixa os arquivos inteiros.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// O que o Windows pede ao provedor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pedido {
    /// Um programa leu um arquivo que ainda não está no disco: entregue estes bytes.
    Buscar(Busca),
    /// O programa desistiu — fechou o arquivo, ou o pedido venceu.
    Cancelar {
        /// A transferência cancelada.
        transferencia: i64,
    },
}

/// Os bytes que o Windows quer de um arquivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busca {
    /// A raiz de sincronia em que o arquivo está.
    pub raiz: PathBuf,
    /// O caminho relativo à raiz, com `/`.
    pub caminho: String,
    /// A conexão com a raiz, para responder.
    pub conexao: i64,
    /// A transferência, para responder.
    pub transferencia: i64,
    /// De onde.
    pub offset: u64,
    /// Quantos bytes.
    pub tamanho: u64,
}

/// O que se sabe de um arquivo numa raiz de sincronia, pelos atributos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Situacao {
    /// O conteúdo ainda não está no disco.
    pub sem_conteudo: bool,
    /// A pessoa pediu "Sempre manter neste dispositivo".
    pub fixado: bool,
    /// A pessoa pediu "Liberar espaço".
    pub liberar: bool,
}

#[cfg(windows)]
pub use windows::{
    Conexao, atualizar_marcador, conectar, criar_marcador, desidratar, desregistrar, entregar,
    falhar, hidratar, limpar_orfas, marcar_em_dia, registrar, reverter, situacao, suportado,
};

#[cfg(target_os = "linux")]
pub use linux::{Montagem, entregar, falhar, marcar_sem_conteudo, montar, sem_conteudo, suportado};

/// Fora do Windows e do Linux não há pasta sob demanda.
#[cfg(not(any(windows, target_os = "linux")))]
#[must_use]
pub const fn suportado(_raiz: &Path) -> bool {
    false
}

/// O identificador da raiz de sincronia de uma pasta, único por usuário e por pasta.
///
/// Formato recomendado pela Microsoft: provedor, conta e raiz separados por `!`, sem espaço.
#[must_use]
pub fn id_da_raiz(usuario: &str, pasta_hex: &str) -> String {
    let limpo: String = usuario
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("InputRemote!{limpo}!{pasta_hex}")
}

/// O caminho relativo à raiz, com `/`, de um caminho absoluto dentro dela. `None` fora dela.
#[must_use]
pub fn relativo(raiz: &Path, caminho: &Path) -> Option<String> {
    let resto: PathBuf = if let Ok(resto) = caminho.strip_prefix(raiz) {
        resto.to_path_buf()
    } else {
        // O Windows não diferencia caixa no caminho: `C:\Users` e `c:\users` são o mesmo lugar.
        let raiz = raiz.to_string_lossy().to_lowercase();
        let texto = caminho.to_string_lossy().into_owned();
        if !texto.to_lowercase().starts_with(&raiz) {
            return None;
        }
        let resto = texto.get(raiz.len()..).unwrap_or_default();
        PathBuf::from(resto.trim_start_matches(['\\', '/']))
    };
    let partes: Vec<String> = resto
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!partes.is_empty()).then(|| partes.join("/"))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_id_da_raiz_nao_tem_espaco_nem_barra() {
        assert_eq!(
            id_da_raiz("Ana Maria", "0a1b"),
            "InputRemote!Ana_Maria!0a1b"
        );
    }

    #[test]
    fn o_relativo_ignora_a_caixa_e_usa_barra() {
        let raiz = Path::new(r"C:\Users\Ana\InputRemote\Projetos");
        let dentro = Path::new(r"c:\users\ana\inputremote\projetos\Relatório\março.xlsx");
        if cfg!(windows) {
            assert_eq!(
                relativo(raiz, dentro).as_deref(),
                Some("Relatório/março.xlsx")
            );
        }
        assert_eq!(relativo(raiz, raiz), None);
    }
}
