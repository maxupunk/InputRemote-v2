//! A cópia grande espera a travessia.
//!
//! Onde o sistema avisa a cópia — o Windows, e o GNOME pelo portal ([ADR-0017]) —, o que se copia
//! sai na hora para o outro computador. Para texto, uma imagem ou um arquivo pequeno é o melhor:
//! quando a pessoa chega lá, já está. Para um arquivo grande, é o oposto. Copiar um vídeo de 4 GB
//! para colar **aqui mesmo**, em outra pasta, punha a rede e o disco do outro computador para
//! trabalhar por minutos, por nada.
//!
//! Então, no aviso de cópia, o que passa de [`LIMITE`] fica: vai quando o cursor atravessar, que é
//! o sinal de que a pessoa vai usar o outro computador ([ADR-0011]). A guarda de eco não o marca
//! como oferecido, e a travessia o leva. Se nem assim era para ir, a notificação e a bandeja têm
//! "Cancelar".
//!
//! Isto vale para quem escolheu "Na hora" nas Preferências. O padrão é "Ao passar o cursor"
//! ([`Escolhas`]): nada sai antes da travessia, e o ajudante nem lê o clipboard no aviso de cópia.
//!
//! [ADR-0011]: ../../../../docs/adr/0011-clipboard-na-travessia.md
//! [ADR-0017]: ../../../../docs/adr/0017-clipboard-do-gnome-pelo-portal.md

use std::path::{Path, PathBuf};

use ir_ipc::Pedido;

/// O que vai na hora da cópia: até 100 MB. Pela rede local isso leva segundos, e cobre a imagem, o
/// documento e a pasta de trabalho; o vídeo, a imagem de disco e a pasta de fotos esperam.
pub(super) const LIMITE: u64 = 100 * 1024 * 1024;

/// Quantos itens se olha antes de desistir de somar: uma pasta com mais que isto é grande pelo
/// número, e varrer uma árvore inteira só para decidir atrasaria a cópia que era pequena.
const ITENS: usize = 10_000;

/// Por que o envio foi pedido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Gatilho {
    /// O sistema avisou a cópia: a pessoa pode estar copiando para colar aqui mesmo.
    Copia,
    /// O cursor saiu para o outro computador: o que está copiado vai junto.
    Travessia,
}

/// O que as Preferências dizem sobre a cópia que sai daqui.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Escolhas {
    /// Copiar e colar ligado.
    ligado: bool,
    /// "Ao passar o cursor": nada sai no aviso de cópia.
    na_travessia: bool,
}

impl Default for Escolhas {
    /// As do serviço recém-instalado, até o estado chegar.
    fn default() -> Self {
        Self {
            ligado: true,
            na_travessia: true,
        }
    }
}

impl Escolhas {
    /// As escolhas, como o serviço as contou.
    pub(super) const fn do_estado(estado: &ir_ipc::Estado) -> Self {
        Self {
            ligado: estado.copiar_e_colar,
            na_travessia: estado.copia_na_travessia,
        }
    }

    /// Se este gatilho lê o clipboard.
    pub(super) fn le(self, gatilho: Gatilho) -> bool {
        self.ligado && (gatilho == Gatilho::Travessia || !self.na_travessia)
    }
}

/// Se este pedido, por este gatilho, deve esperar a travessia.
pub(super) fn espera_a_travessia(pedido: &Pedido, gatilho: Gatilho) -> bool {
    match (pedido, gatilho) {
        (Pedido::EnviarArquivos { caminhos }, Gatilho::Copia) => {
            passa_de(caminhos.iter().map(PathBuf::from).collect(), LIMITE)
        }
        _ => false,
    }
}

/// Se os arquivos, somados com as pastas inteiras, passam de `limite`. Para de contar assim que
/// passa — ou quando há itens demais para valer a pena contar.
///
/// O que não se lê conta zero: quem decide se dá para mandar é o serviço, com a permissão de quem
/// pediu, e não esta soma.
fn passa_de(mut pendentes: Vec<PathBuf>, limite: u64) -> bool {
    let mut total = 0u64;
    let mut vistos = 0usize;
    while let Some(caminho) = pendentes.pop() {
        vistos += 1;
        if vistos > ITENS {
            return true;
        }
        // Sem seguir atalhos: o serviço também não os segue, e um atalho para a raiz do disco
        // faria a soma varrer tudo.
        let Ok(dados) = std::fs::symlink_metadata(&caminho) else {
            continue;
        };
        if dados.is_dir() {
            pendentes.extend(conteudo_de(&caminho));
        } else {
            total = total.saturating_add(dados.len());
            if total > limite {
                return true;
            }
        }
    }
    false
}

fn conteudo_de(pasta: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(pasta)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entrada| entrada.path())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("ir-grande-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pasta);
        std::fs::create_dir_all(pasta.join("dentro")).unwrap();
        pasta
    }

    fn enviar(caminho: &Path) -> Pedido {
        Pedido::EnviarArquivos {
            caminhos: vec![caminho.to_string_lossy().into_owned()],
        }
    }

    #[test]
    fn ao_passar_o_cursor_so_a_travessia_le_e_desligado_nada_le() {
        let mut estado = ir_ipc::Estado::recem_instalado(
            ir_ipc::Maquina([0; 16]),
            ir_ipc::Nome::coagido("bancada"),
        );
        let le = |estado: &ir_ipc::Estado, gatilho| Escolhas::do_estado(estado).le(gatilho);
        assert_eq!(Escolhas::do_estado(&estado), Escolhas::default());
        assert!(!le(&estado, Gatilho::Copia), "o padrão é esperar o cursor");
        assert!(le(&estado, Gatilho::Travessia));

        estado.copia_na_travessia = false;
        assert!(le(&estado, Gatilho::Copia) && le(&estado, Gatilho::Travessia));

        estado.copiar_e_colar = false;
        assert!(!le(&estado, Gatilho::Copia) && !le(&estado, Gatilho::Travessia));
    }

    #[test]
    fn soma_as_pastas_por_dentro_e_para_quando_passa() {
        let pasta = pasta_de_teste("soma");
        std::fs::write(pasta.join("a.bin"), vec![0u8; 600]).unwrap();
        std::fs::write(pasta.join("dentro").join("b.bin"), vec![0u8; 600]).unwrap();
        assert!(passa_de(vec![pasta.clone()], 1_000));
        assert!(!passa_de(vec![pasta.clone()], 1_200));
        assert!(!passa_de(vec![pasta.join("a.bin")], 1_000));
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn o_que_nao_existe_nao_pesa() {
        assert!(!passa_de(vec![PathBuf::from("/nao/existe/mesmo")], 0));
    }

    #[test]
    fn so_a_copia_de_arquivo_grande_espera_e_a_travessia_leva() {
        let pasta = pasta_de_teste("gatilho");
        let arquivo = pasta.join("video.bin");
        std::fs::File::create(&arquivo)
            .unwrap()
            .set_len(LIMITE + 1)
            .unwrap();
        assert!(espera_a_travessia(&enviar(&arquivo), Gatilho::Copia));
        assert!(!espera_a_travessia(&enviar(&arquivo), Gatilho::Travessia));
        let texto = Pedido::OferecerTexto(ir_ipc::TextoDoClipboard::new("x".to_owned()).unwrap());
        assert!(!espera_a_travessia(&texto, Gatilho::Copia));
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
