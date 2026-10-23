//! Os tipos MIME do portal, nos dois sentidos.
//!
//! O portal não converte nada: quem publica diz quais tipos tem e serve os bytes de cada um quando
//! alguém cola; quem lê pede um tipo e recebe os bytes dele. Então a escolha de tipos que o
//! `wl-copy` e o `wl-paste` faziam por nós passa a ser daqui.

use std::path::PathBuf;

use crate::conteudo::Conteudo;
use crate::uri;

/// Lista de arquivos do XDG ([RFC 2483](https://www.rfc-editor.org/rfc/rfc2483)).
const LISTA_DE_URIS: &str = "text/uri-list";
/// A lista de arquivos do Nautilus: `copy` ou `cut` na primeira linha, URIs nas outras. O Nautilus
/// cola por ela antes da `text/uri-list` ([ADR-0011](../../../../../docs/adr/0011-clipboard-na-travessia.md)).
const ARQUIVOS_DO_GNOME: &str = "x-special/gnome-copied-files";
/// A imagem, na forma canônica do protocolo.
const PNG: &str = "image/png";
/// Os nomes de texto, do preferido ao último recurso. Os dois últimos são os do X11, que aplicativos
/// pelo `XWayland` ainda oferecem sozinhos.
const TEXTOS: [&str; 5] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "STRING",
    "TEXT",
];

/// O que tentar ler quando ainda não se sabe o que há: antes do primeiro aviso de mudança, o portal
/// não diz os tipos. Na ordem de [`escolher`].
pub(super) const TENTATIVAS: [&str; 4] = [LISTA_DE_URIS, ARQUIVOS_DO_GNOME, TEXTOS[0], PNG];

/// Cada tipo que este conteúdo oferece, com os bytes que ele serve.
pub(super) fn ofertas(conteudo: &Conteudo) -> Vec<(&'static str, Vec<u8>)> {
    match conteudo {
        Conteudo::Texto(texto) => TEXTOS
            .iter()
            .map(|tipo| (*tipo, texto.clone().into_bytes()))
            .collect(),
        Conteudo::Arquivos(caminhos) => {
            let textos: Vec<String> = caminhos
                .iter()
                .map(|caminho| caminho.to_string_lossy().into_owned())
                .collect();
            let uris: Vec<String> = textos.iter().map(|caminho| uri::uri_de(caminho)).collect();
            let do_gnome = format!("copy\n{}", uris.join("\n"));
            vec![
                (ARQUIVOS_DO_GNOME, do_gnome.into_bytes()),
                (LISTA_DE_URIS, uri::montar_lista(&textos).into_bytes()),
            ]
        }
        Conteudo::Imagem(png) => vec![(PNG, png.clone())],
    }
}

/// Qual dos tipos oferecidos ler.
///
/// Arquivos antes de texto, porque o gerenciador de arquivos oferece os dois e ler o texto faria de
/// "copiei um arquivo" um "copiei um nome"; imagem depois de texto, porque a planilha oferece uma
/// figura das células junto com elas. A mesma ordem do `wl-clipboard` e do Windows.
pub(super) fn escolher(oferecidos: &[String]) -> Option<&'static str> {
    let tem = |procurado: &str| {
        oferecidos
            .iter()
            .any(|tipo| tipo.trim().eq_ignore_ascii_case(procurado))
    };
    [LISTA_DE_URIS, ARQUIVOS_DO_GNOME]
        .into_iter()
        .chain(TEXTOS)
        .chain([PNG])
        .find(|tipo| tem(tipo))
}

/// Os bytes lidos de um tipo, como conteúdo. `None` para o vazio, que não é cópia que valha contar.
pub(super) fn conteudo(tipo: &str, bytes: &[u8]) -> Option<Conteudo> {
    if bytes.is_empty() {
        return None;
    }
    let texto = || String::from_utf8_lossy(bytes);
    let conteudo = match tipo {
        LISTA_DE_URIS => arquivos(&uri::lista(&texto())),
        ARQUIVOS_DO_GNOME => {
            let texto = texto();
            let resto: Vec<&str> = texto.lines().skip(1).collect();
            arquivos(&uri::lista(&resto.join("\n")))
        }
        PNG => Conteudo::Imagem(bytes.to_vec()),
        _ => Conteudo::texto(&texto()),
    };
    (!conteudo.vazio()).then_some(conteudo)
}

/// Uma lista de arquivos, a partir de caminhos já decodificados.
fn arquivos(caminhos: &[String]) -> Conteudo {
    Conteudo::Arquivos(caminhos.iter().map(PathBuf::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tipos(lista: &[&str]) -> Vec<String> {
        lista.iter().map(|tipo| (*tipo).to_owned()).collect()
    }

    #[test]
    fn arquivos_ganham_do_texto_e_texto_ganha_da_imagem() {
        let nautilus = tipos(&[
            "text/plain",
            "x-special/gnome-copied-files",
            "text/uri-list",
        ]);
        assert_eq!(escolher(&nautilus), Some(LISTA_DE_URIS));
        let planilha = tipos(&["image/png", "text/plain;charset=utf-8"]);
        assert_eq!(escolher(&planilha), Some("text/plain;charset=utf-8"));
        assert_eq!(escolher(&tipos(&["image/png"])), Some(PNG));
    }

    #[test]
    fn o_texto_do_x11_e_o_charset_em_maiusculas_tambem_valem() {
        assert_eq!(escolher(&tipos(&["UTF8_STRING"])), Some("UTF8_STRING"));
        assert_eq!(
            escolher(&tipos(&["text/plain;charset=UTF-8"])),
            Some("text/plain;charset=utf-8")
        );
    }

    #[test]
    fn formato_privado_nao_e_para_nos() {
        assert_eq!(escolher(&tipos(&["application/x-gimp-layer"])), None);
        assert_eq!(escolher(&[]), None);
    }

    #[test]
    fn os_arquivos_publicados_voltam_iguais_pelos_dois_tipos() {
        let copia = Conteudo::Arquivos(vec![
            PathBuf::from("/home/eu/relatório de janeiro.pdf"),
            PathBuf::from("/home/eu/fotos"),
        ]);
        let ofertas = ofertas(&copia);
        assert_eq!(ofertas.len(), 2);
        for (tipo, bytes) in &ofertas {
            assert_eq!(conteudo(tipo, bytes).as_ref(), Some(&copia), "{tipo}");
        }
        let (_, do_gnome) = ofertas
            .iter()
            .find(|(tipo, _)| *tipo == ARQUIVOS_DO_GNOME)
            .expect("o tipo do Nautilus");
        assert!(do_gnome.starts_with(b"copy\nfile:///home/eu/relat%C3%B3rio%20de"));
    }

    #[test]
    fn o_texto_e_oferecido_em_todos_os_nomes_e_volta_normalizado() {
        let ofertas = ofertas(&Conteudo::Texto("olá\nmundo".to_owned()));
        assert_eq!(ofertas.len(), TEXTOS.len());
        assert_eq!(
            conteudo("text/plain", b"ol\xc3\xa1\r\nmundo"),
            Some(Conteudo::Texto("olá\nmundo".to_owned()))
        );
    }

    #[test]
    fn vazio_nao_e_copia() {
        assert_eq!(conteudo("text/plain", b""), None);
        assert_eq!(conteudo(LISTA_DE_URIS, b"# so comentario\r\n"), None);
    }
}
