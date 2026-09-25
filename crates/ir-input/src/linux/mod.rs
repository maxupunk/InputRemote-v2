//! O backend Linux: injeção por `uinput`, e captura por `evdev`.

#![allow(unreachable_pub)]

mod aceleracao;
pub mod captura;
pub mod keymap;
mod touchpad;
mod traducao;
pub mod uinput;

/// O começo do nome de todo dispositivo virtual do produto.
///
/// A captura ignora quem começa assim: ler o próprio teclado e o próprio ponteiro virtuais faria
/// cada injeção voltar como entrada local. Os nomes nascem de [`nome_virtual`], e a captura
/// pergunta a [`e_virtual_do_produto`] — o prefixo existe num lugar só.
const PREFIXO_DOS_VIRTUAIS: &str = "InputRemote";

/// O nome de um dispositivo virtual do produto: o prefixo e o papel.
fn nome_virtual(papel: &str) -> String {
    format!("{PREFIXO_DOS_VIRTUAIS} {papel}")
}

/// Se este é o nome de um dispositivo virtual do produto.
fn e_virtual_do_produto(nome: &str) -> bool {
    nome.starts_with(PREFIXO_DOS_VIRTUAIS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_captura_reconhece_os_virtuais_que_o_injetor_cria() {
        for papel in ["Keyboard", "Pointer"] {
            assert!(e_virtual_do_produto(&nome_virtual(papel)), "{papel}");
        }
        assert!(!e_virtual_do_produto("AT Translated Set 2 keyboard"));
    }
}
