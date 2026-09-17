//! Como a entrega aparece no destino.
//!
//! Decisão pequena e inteiramente pura, separada da recepção porque é a única parte dela que o
//! usuário **vê**: é o nome e a forma do que aparece na pasta de recebidos quando ele cola.
//!
//! O erro que este módulo existe para não repetir estava na primeira versão e apareceu no primeiro
//! teste de travessia: publicar a montagem inteira produz um nível a mais. Copiar `relatório` dava
//! `recebidos/relatório/relatório/a.pdf`, porque a montagem já tem a árvore copiada dentro dela.

use ir_proto::message::ManifestItem;

/// O que publicar, e com que nome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publicacao {
    /// Uma raiz só: ela mesma é publicada, e o destino espelha a origem exatamente.
    Entrada(String),
    /// Várias raízes: não há nome natural, então elas vão dentro de uma pasta que as agrupa.
    Agrupadas(String),
}

/// Decide, a partir do manifesto, o que publicar e com que nome.
///
/// Não há campo de nome no canal 5, e acrescentar um seria mudar o formato de fio para transportar
/// o que já está nos caminhos: o primeiro componente de cada item é a raiz que o usuário copiou.
#[must_use]
pub fn como_publicar(itens: &[ManifestItem]) -> Publicacao {
    let mut raizes = itens
        .iter()
        .map(|item| item.path.split('/').next().unwrap_or_default())
        .filter(|nome| !nome.is_empty())
        .collect::<Vec<_>>();
    raizes.sort_unstable();
    raizes.dedup();

    match raizes.as_slice() {
        [uma] => Publicacao::Entrada((*uma).to_owned()),
        [primeira, ..] => Publicacao::Agrupadas(format!("{primeira} e outros")),
        [] => Publicacao::Agrupadas("recebido".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(caminho: &str, pasta: bool) -> ManifestItem {
        ManifestItem {
            path: caminho.to_owned(),
            size: 0,
            is_dir: pasta,
        }
    }

    #[test]
    fn uma_arvore_e_publicada_por_ela_mesma() {
        let itens = vec![
            item("relatorio", true),
            item("relatorio/a.pdf", false),
            item("relatorio/anexos", true),
        ];
        assert_eq!(
            como_publicar(&itens),
            Publicacao::Entrada("relatorio".to_owned())
        );
    }

    #[test]
    fn um_arquivo_solto_e_publicado_por_ele_mesmo() {
        // Colar um arquivo tem de dar um arquivo, e não uma pasta com um arquivo dentro.
        assert_eq!(
            como_publicar(&[item("nota.txt", false)]),
            Publicacao::Entrada("nota.txt".to_owned())
        );
    }

    #[test]
    fn varias_raizes_ganham_uma_pasta_que_as_agrupa() {
        let itens = vec![item("a.txt", false), item("b.txt", false)];
        assert_eq!(
            como_publicar(&itens),
            Publicacao::Agrupadas("a.txt e outros".to_owned())
        );
    }

    #[test]
    fn a_ordem_dos_itens_nao_muda_a_decisao() {
        // O manifesto não promete ordem, e o nome do que o usuário vê não pode depender dela.
        let subindo = vec![item("a", true), item("a/x", false), item("a/y", false)];
        let descendo = vec![item("a/y", false), item("a/x", false), item("a", true)];
        assert_eq!(como_publicar(&subindo), como_publicar(&descendo));
    }

    #[test]
    fn um_manifesto_vazio_nao_causa_panico() {
        assert_eq!(
            como_publicar(&[]),
            Publicacao::Agrupadas("recebido".to_owned())
        );
    }
}
