//! Caminhos locais e URIs `file://`, nos dois sentidos.
//!
//! No Wayland uma lista de arquivos no clipboard é `text/uri-list` ([RFC 2483]): uma URI por linha,
//! e os bytes que não são seguros numa URI vão como `%XX`. O Nautilus põe `relatório de janeiro`
//! como `relat%C3%B3rio%20de%20janeiro`.
//!
//! Errar isto não dá erro: dá um caminho que **não existe**. Decodificar sem tratar o `%20` faria o
//! manifesto procurar `relatório%20de%20janeiro` no disco, e a transferência falharia dizendo que o
//! arquivo sumiu — com o arquivo lá, intacto.
//!
//! Puro e compilado em toda plataforma, para os testes rodarem também onde o backend não roda.
//!
//! [RFC 2483]: https://www.rfc-editor.org/rfc/rfc2483

/// O caminho local de uma URI `file://`, ou `None` se ela não for de arquivo local.
///
/// Aceita `file:///caminho` e `file://localhost/caminho`. Recusa `file://outra-maquina/…`: um
/// arquivo em outro computador não é algo que esta máquina possa ler para mandar.
///
/// Recusa também o que, depois de decodificado, não é UTF-8 — o campo do manifesto é texto, e o
/// `ir-files` já recusa nome que não seja ([log 31](../../../docs/logs/31-o-motor-de-transferencia.md)).
#[must_use]
pub fn caminho_de(uri: &str) -> Option<String> {
    let resto = uri.trim().strip_prefix("file://")?;
    let caminho = if let Some(local) = resto.strip_prefix("localhost") {
        local
    } else if resto.starts_with('/') {
        resto
    } else {
        return None; // `file://servidor/…`
    };
    let bytes = decodificar(caminho)?;
    String::from_utf8(bytes).ok()
}

/// A URI `file://` de um caminho local absoluto.
#[must_use]
pub fn uri_de(caminho: &str) -> String {
    let mut saida = String::with_capacity(caminho.len() + 8);
    saida.push_str("file://");
    for byte in caminho.bytes() {
        if seguro(byte) {
            saida.push(char::from(byte));
        } else {
            saida.push('%');
            saida.push(hexa(byte >> 4));
            saida.push(hexa(byte & 0x0f));
        }
    }
    saida
}

/// Os caminhos de uma `text/uri-list` inteira.
///
/// Linha começando com `#` é comentário, pela RFC. Linha vazia é ignorada. O Nautilus às vezes
/// termina com `\r\n`, e o `trim` de [`caminho_de`] cuida disso.
#[must_use]
pub fn lista(texto: &str) -> Vec<String> {
    texto
        .lines()
        .map(str::trim)
        .filter(|linha| !linha.is_empty() && !linha.starts_with('#'))
        .filter_map(caminho_de)
        .collect()
}

/// A `text/uri-list` de uma lista de caminhos.
#[must_use]
pub fn montar_lista(caminhos: &[String]) -> String {
    // CRLF é o separador da RFC 2483, e é o que os leitores esperam.
    caminhos
        .iter()
        .map(|caminho| uri_de(caminho))
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Os bytes que podem ir crus numa URI de arquivo: os não reservados, mais a barra.
const fn seguro(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
}

fn hexa(nibble: u8) -> char {
    char::from(
        b"0123456789ABCDEF"
            .get(usize::from(nibble))
            .copied()
            .unwrap_or(b'0'),
    )
}

/// Troca cada `%XX` pelo byte que ele representa. `None` para `%` malformado.
fn decodificar(texto: &str) -> Option<Vec<u8>> {
    let mut saida = Vec::with_capacity(texto.len());
    let mut bytes = texto.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let alto = valor_hexa(bytes.next()?)?;
            let baixo = valor_hexa(bytes.next()?)?;
            saida.push((alto << 4) | baixo);
        } else {
            saida.push(byte);
        }
    }
    Some(saida)
}

const fn valor_hexa(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn um_caminho_simples_vai_e_volta() {
        let uri = uri_de("/home/maxuel/nota.txt");
        assert_eq!(uri, "file:///home/maxuel/nota.txt");
        assert_eq!(caminho_de(&uri).as_deref(), Some("/home/maxuel/nota.txt"));
    }

    #[test]
    fn espaco_e_acento_sao_codificados_como_o_nautilus_faz() {
        // O caso que o módulo existe para acertar. Sem decodificar, o manifesto procuraria o nome
        // com `%20` no disco e diria que o arquivo sumiu.
        let caminho = "/home/maxuel/relatório de janeiro";
        let uri = uri_de(caminho);
        assert_eq!(uri, "file:///home/maxuel/relat%C3%B3rio%20de%20janeiro");
        assert_eq!(caminho_de(&uri).as_deref(), Some(caminho));
    }

    #[test]
    fn a_ida_e_volta_preserva_qualquer_caminho_utf8() {
        for caminho in [
            "/a",
            "/com espaço/e acentuação/arquivo (1).txt",
            "/símbolos #, ?, &, %, + e =",
            "/emoji/🙂.png",
            "/tmp/100%",
        ] {
            assert_eq!(
                caminho_de(&uri_de(caminho)).as_deref(),
                Some(caminho),
                "{caminho}"
            );
        }
    }

    #[test]
    fn localhost_e_aceito_e_outra_maquina_nao() {
        assert_eq!(
            caminho_de("file://localhost/etc/hostname").as_deref(),
            Some("/etc/hostname")
        );
        // Um arquivo noutro computador não é algo que esta máquina possa ler para mandar.
        assert_eq!(caminho_de("file://servidor/compartilhado/x"), None);
    }

    #[test]
    fn o_que_nao_e_arquivo_e_recusado() {
        for uri in ["https://example.com/x", "/sem/esquema", "", "file:"] {
            assert_eq!(caminho_de(uri), None, "{uri}");
        }
    }

    #[test]
    fn porcentagem_malformada_e_recusada_e_nao_adivinhada() {
        for uri in ["file:///a%2", "file:///a%zz", "file:///a%"] {
            assert_eq!(caminho_de(uri), None, "{uri}");
        }
    }

    #[test]
    fn bytes_que_nao_sao_utf8_sao_recusados() {
        // O manifesto é texto; aceitar aqui só moveria a recusa para mais longe.
        assert_eq!(caminho_de("file:///a%FFb"), None);
    }

    #[test]
    fn a_lista_ignora_comentario_linha_vazia_e_crlf() {
        let texto = "# comentário da RFC\r\nfile:///a.txt\r\n\r\nfile:///b%20c.txt\r\n";
        assert_eq!(lista(texto), vec!["/a.txt", "/b c.txt"]);
    }

    #[test]
    fn a_lista_montada_e_lida_de_volta() {
        let caminhos = vec!["/um.txt".to_owned(), "/pasta com espaço".to_owned()];
        assert_eq!(lista(&montar_lista(&caminhos)), caminhos);
    }
}
