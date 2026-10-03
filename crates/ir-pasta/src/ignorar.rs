//! O que nunca sincroniza.
//!
//! Três tipos de arquivo ficam de fora, e cada um por um motivo diferente:
//!
//! - **O que um programa cria enquanto trabalha.** O Word abre `~$relatório.docx` ao lado do
//!   documento e o apaga ao fechar; o LibreOffice faz `.~lock.relatório.odt#`; o vim, `.swp` e um
//!   arquivo de teste chamado `4913`; o navegador baixa em `.crdownload` e só renomeia no fim.
//!   Sincronizar isso é mandar ao outro computador um arquivo que some em segundos — e, no caso do
//!   arquivo de trava, travar o documento lá também.
//! - **O que o sistema escreve sozinho em qualquer pasta.** `Thumbs.db`, `desktop.ini`,
//!   `.DS_Store`: cada lado tem o seu, e eles brigariam em conflito para sempre.
//! - **O que é da própria pasta compartilhada.** `.inputremote` guarda a lixeira e a montagem.
//!
//! A conferência é por componente: um arquivo dentro de uma pasta ignorada também fica de fora.

/// O nome da pasta de controle, dentro de cada pasta compartilhada.
pub const PASTA_DE_CONTROLE: &str = ".inputremote";

/// Se o caminho relativo, com `/`, fica de fora da sincronia.
#[must_use]
pub fn ignorar_caminho(caminho: &str) -> bool {
    caminho.split('/').any(ignorar_nome)
}

/// Se um nome — um componente só — fica de fora da sincronia.
#[must_use]
pub fn ignorar_nome(nome: &str) -> bool {
    let minusculo = nome.to_lowercase();
    let n = minusculo.as_str();
    n == PASTA_DE_CONTROLE || do_sistema(n) || de_trava(n) || temporario(n)
}

/// O que o sistema escreve sozinho. Comparado sem caixa: no Windows `THUMBS.DB` é o mesmo arquivo.
fn do_sistema(n: &str) -> bool {
    matches!(
        n,
        "thumbs.db"
            | "ehthumbs.db"
            | "desktop.ini"
            | ".ds_store"
            | "$recycle.bin"
            | "system volume information"
    ) || n.starts_with(".trash-")
}

/// Os arquivos de trava dos editores: quem os sincroniza trava o documento no outro computador.
fn de_trava(n: &str) -> bool {
    n.starts_with("~$") || (n.starts_with(".~lock.") && n.ends_with('#')) || n.starts_with(".#")
}

/// O que um programa cria enquanto trabalha e apaga ou renomeia em seguida.
fn temporario(n: &str) -> bool {
    const EXTENSOES: [&str; 8] = [
        ".tmp",
        ".temp",
        ".swp",
        ".swo",
        ".swx",
        ".part",
        ".partial",
        ".crdownload",
    ];
    n == "4913" || n.ends_with('~') || EXTENSOES.iter().any(|ext| n.ends_with(ext))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn os_temporarios_do_office_do_libreoffice_e_do_vim_ficam_de_fora() {
        for nome in [
            "~$relatório.docx",
            "~WRL0001.tmp",
            ".~lock.planilha.ods#",
            ".notas.txt.swp",
            "4913",
            "notas.txt~",
            ".#emacs-lock",
            "filme.mkv.crdownload",
            "download.part",
        ] {
            assert!(ignorar_nome(nome), "{nome} deveria ficar de fora");
        }
    }

    #[test]
    fn os_arquivos_do_sistema_ficam_de_fora_em_qualquer_caixa() {
        for nome in [
            "Thumbs.db",
            "THUMBS.DB",
            "desktop.ini",
            ".DS_Store",
            ".Trash-1000",
        ] {
            assert!(ignorar_nome(nome), "{nome}");
        }
    }

    #[test]
    fn o_conteudo_de_uma_pasta_ignorada_tambem_fica_de_fora() {
        assert!(ignorar_caminho(".inputremote/lixeira/2026-10-02/a.txt"));
        assert!(ignorar_caminho("fotos/.Trash-1000/x.jpg"));
        assert!(!ignorar_caminho("fotos/2026/praia.jpg"));
    }

    #[test]
    fn o_que_o_usuario_criou_viaja() {
        for nome in [
            "relatório.docx",
            "planilha.ods",
            "lock.txt",
            "template.temporada.pdf",
            "$dinheiro.xlsx",
            ".bashrc",
            "a~b.txt",
            "49130",
        ] {
            assert!(!ignorar_nome(nome), "{nome} deveria viajar");
        }
    }
}
