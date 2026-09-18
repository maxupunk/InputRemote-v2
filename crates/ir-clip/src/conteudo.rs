//! O que há num clipboard, na forma canônica do protocolo.
//!
//! # Canônico significa uma forma só
//!
//! O texto aqui é **sempre** LF, nunca CRLF ([01, §3.2](../../../docs/01-visao-e-escopo.md)). A
//! conversão para a quebra nativa é do backend, na hora de publicar, e a conversão de volta é do
//! backend, na hora de ler.
//!
//! Isso não é preferência de formato: é o que faz a guarda de eco funcionar. Se este tipo
//! guardasse o que o sistema deu, o texto que publicamos com CRLF voltaria do clipboard com CRLF e
//! teria um resumo diferente do que publicamos — a guarda não reconheceria a própria cópia, e o
//! Ctrl+C viraria um laço infinito entre os dois computadores.
//!
//! A imagem é PNG, qualquer que seja o formato nativo. A lista de arquivos é de caminhos locais.

use std::path::{Path, PathBuf};

use ir_proto::message::ClipKind;

/// O conteúdo de um clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Conteudo {
    /// Texto UTF-8, **sempre com LF**.
    Texto(String),
    /// Imagem em PNG.
    Imagem(Vec<u8>),
    /// Arquivos e pastas, por caminho local.
    Arquivos(Vec<PathBuf>),
}

impl Conteudo {
    /// Texto, com as quebras já normalizadas.
    ///
    /// O construtor a usar sempre que o texto vem do sistema operacional. Passar direto pela
    /// variante deixaria um CRLF entrar, e ele estragaria o resumo.
    #[must_use]
    pub fn texto(bruto: &str) -> Self {
        Self::Texto(normalizar_quebras(bruto))
    }

    /// Que tipo de conteúdo é este, no vocabulário do protocolo.
    #[must_use]
    pub const fn tipo(&self) -> ClipKind {
        match self {
            Self::Texto(_) => ClipKind::Text,
            Self::Imagem(_) => ClipKind::Image,
            Self::Arquivos(_) => ClipKind::Files,
        }
    }

    /// Quanto isto ocupa, para decidir por qual canal vai.
    ///
    /// Para arquivos é o tamanho da **lista**, e não o dos arquivos: quem sabe o tamanho do
    /// conteúdo é o manifesto, e ele é montado depois, pelo `ir-files`.
    #[must_use]
    pub fn tamanho(&self) -> usize {
        match self {
            Self::Texto(texto) => texto.len(),
            Self::Imagem(bytes) => bytes.len(),
            Self::Arquivos(caminhos) => caminhos
                .iter()
                .map(|caminho| caminho.as_os_str().len())
                .sum(),
        }
    }

    /// Se não há nada de fato.
    ///
    /// Um clipboard esvaziado não é conteúdo novo a oferecer. Sem isto, apagar o clipboard mandaria
    /// uma oferta vazia ao par, que apagaria o dele.
    #[must_use]
    pub fn vazio(&self) -> bool {
        match self {
            Self::Texto(texto) => texto.is_empty(),
            Self::Imagem(bytes) => bytes.is_empty(),
            Self::Arquivos(caminhos) => caminhos.is_empty(),
        }
    }

    /// O resumo BLAKE3 da forma canônica.
    ///
    /// É a identidade do conteúdo, e é sobre ela que a guarda de eco decide. Dois detalhes que
    /// importam:
    ///
    /// - o **tipo entra no resumo**, então um texto e uma imagem nunca colidem;
    /// - para arquivos, a lista entra **na ordem em que está**, porque a ordem é o que o usuário
    ///   selecionou e o que o destino vai ver.
    #[must_use]
    pub fn resumo(&self) -> [u8; 32] {
        let mut resumo = blake3::Hasher::new();
        // Um byte de tipo na frente: sem ele, um texto `"a"` e uma imagem cujos bytes fossem `a`
        // teriam o mesmo resumo, e a guarda de eco confundiria um com o outro.
        resumo.update(&[especie(self.tipo())]);
        match self {
            Self::Texto(texto) => {
                resumo.update(texto.as_bytes());
            }
            Self::Imagem(bytes) => {
                resumo.update(bytes);
            }
            Self::Arquivos(caminhos) => {
                for caminho in caminhos {
                    resumo.update(caminho.to_string_lossy().as_bytes());
                    // Separador que não pode aparecer num caminho, para `["ab"]` e `["a","b"]` não
                    // darem o mesmo resumo.
                    resumo.update(&[0]);
                }
            }
        }
        *resumo.finalize().as_bytes()
    }

    /// Os caminhos, quando é uma lista de arquivos.
    #[must_use]
    pub fn caminhos(&self) -> &[PathBuf] {
        match self {
            Self::Arquivos(caminhos) => caminhos,
            _ => &[],
        }
    }
}

/// O byte que representa o tipo dentro do resumo.
///
/// Não é o discriminante do fio: este número nunca sai desta máquina, e amarrá-lo ao formato de fio
/// criaria uma dependência que ninguém espera.
const fn especie(tipo: ClipKind) -> u8 {
    match tipo {
        ClipKind::Text => 1,
        ClipKind::Image => 2,
        ClipKind::Files => 3,
        // O enum é não exaustivo; um tipo novo é um tipo novo, e não um dos três.
        _ => 0,
    }
}

/// Troca CRLF e CR soltos por LF.
///
/// Os três aparecem na prática: CRLF do Windows, LF do Linux, e CR sozinho em texto vindo de
/// aplicativos antigos. Sem tratar o CR solto, ele viajaria e apareceria como linha que não quebra
/// do outro lado.
#[must_use]
pub fn normalizar_quebras(bruto: &str) -> String {
    if !bruto.contains('\r') {
        // O caminho comum, e sem alocar mais do que o necessário.
        return bruto.to_owned();
    }
    let mut saida = String::with_capacity(bruto.len());
    let mut anterior_era_cr = false;
    for c in bruto.chars() {
        match c {
            '\r' => {
                saida.push('\n');
                anterior_era_cr = true;
            }
            '\n' if anterior_era_cr => anterior_era_cr = false, // o LF do CRLF já foi contado
            outro => {
                saida.push(outro);
                anterior_era_cr = false;
            }
        }
    }
    saida
}

/// Troca LF pela quebra nativa desta plataforma, para publicar.
#[must_use]
pub fn quebras_nativas(canonico: &str) -> String {
    if cfg!(windows) {
        canonico.replace('\n', "\r\n")
    } else {
        canonico.to_owned()
    }
}

/// Se um caminho pode ser oferecido ao par.
///
/// Recusa o que não existe. Não recusa por tipo: quem decide o que é enviável é o `ir-files`, ao
/// montar o manifesto, e duplicar a regra aqui as faria divergir.
#[must_use]
pub fn oferecivel(caminho: &Path) -> bool {
    caminho.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_texto_chega_sempre_em_lf() {
        for bruto in ["a\r\nb", "a\rb", "a\nb"] {
            assert_eq!(Conteudo::texto(bruto), Conteudo::Texto("a\nb".to_owned()));
        }
    }

    #[test]
    fn a_ida_e_volta_de_quebras_preserva_o_canonico() {
        // O ciclo que a guarda de eco depende: canônico → nativo → canônico dá o mesmo texto. Se
        // não desse, publicar a própria cópia recebida geraria um resumo novo e o Ctrl+C viraria
        // um laço entre as duas máquinas.
        let canonico = "linha um\nlinha dois\n\nfim";
        let nativo = quebras_nativas(canonico);
        assert_eq!(normalizar_quebras(&nativo), canonico);
    }

    #[test]
    fn um_cr_solto_nao_sobrevive() {
        assert_eq!(normalizar_quebras("a\rb\r"), "a\nb\n");
    }

    #[test]
    fn crlf_nao_vira_duas_quebras() {
        assert_eq!(normalizar_quebras("a\r\n\r\nb"), "a\n\nb");
    }

    #[test]
    fn texto_sem_cr_passa_intacto() {
        let limpo = "nada a fazer aqui\ncom acentuação 🙂";
        assert_eq!(normalizar_quebras(limpo), limpo);
    }

    #[test]
    fn o_tipo_entra_no_resumo() {
        // Sem o byte de tipo, estes dois colidiriam — e a guarda de eco trataria uma imagem como
        // se fosse o texto que ela acabou de publicar.
        let texto = Conteudo::Texto("a".to_owned());
        let imagem = Conteudo::Imagem(b"a".to_vec());
        assert_ne!(texto.resumo(), imagem.resumo());
    }

    #[test]
    fn a_lista_de_arquivos_tem_separador_no_resumo() {
        // `["ab"]` e `["a", "b"]` são seleções diferentes, e precisam de resumos diferentes.
        let juntos = Conteudo::Arquivos(vec![PathBuf::from("ab")]);
        let separados = Conteudo::Arquivos(vec![PathBuf::from("a"), PathBuf::from("b")]);
        assert_ne!(juntos.resumo(), separados.resumo());
    }

    #[test]
    fn a_ordem_dos_arquivos_muda_o_resumo() {
        // É a ordem que o usuário selecionou, e é a que o destino vai ver.
        let ab = Conteudo::Arquivos(vec![PathBuf::from("a"), PathBuf::from("b")]);
        let ba = Conteudo::Arquivos(vec![PathBuf::from("b"), PathBuf::from("a")]);
        assert_ne!(ab.resumo(), ba.resumo());
    }

    #[test]
    fn o_resumo_e_estavel_para_o_mesmo_conteudo() {
        let a = Conteudo::texto("mesmo texto\n");
        let b = Conteudo::texto("mesmo texto\r\n");
        assert_eq!(a.resumo(), b.resumo(), "as quebras já foram normalizadas");
    }

    #[test]
    fn vazio_e_vazio_em_todos_os_tipos() {
        assert!(Conteudo::Texto(String::new()).vazio());
        assert!(Conteudo::Imagem(Vec::new()).vazio());
        assert!(Conteudo::Arquivos(Vec::new()).vazio());
        assert!(!Conteudo::texto("x").vazio());
    }

    #[test]
    fn o_tipo_corresponde_ao_do_protocolo() {
        assert_eq!(Conteudo::texto("x").tipo(), ClipKind::Text);
        assert_eq!(Conteudo::Imagem(vec![1]).tipo(), ClipKind::Image);
        assert_eq!(
            Conteudo::Arquivos(vec![PathBuf::from("x")]).tipo(),
            ClipKind::Files
        );
    }

    #[test]
    fn so_arquivos_tem_caminhos() {
        assert!(Conteudo::texto("x").caminhos().is_empty());
        assert_eq!(
            Conteudo::Arquivos(vec![PathBuf::from("x")])
                .caminhos()
                .len(),
            1
        );
    }
}
