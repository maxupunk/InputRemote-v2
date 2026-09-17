//! A política de aceitar ou recusar uma transferência, **antes** de materializar qualquer coisa.
//!
//! Função pura. É a ordem das perguntas que importa aqui, e ela não é arbitrária:
//!
//! 1. **Permissão primeiro.** Se este par não tem autorização para mandar arquivo, a resposta é a
//!    mesma qualquer que seja o tamanho. Perguntar da cota antes contaria ao par qual é a cota, e
//!    a permissão de arquivos é separada e revogável por par
//!    ([04, §2](../../../docs/04-seguranca.md)).
//! 2. **Contagem antes de percorrer.** Conferir o número de itens custa uma comparação; conferir
//!    os caminhos custa percorrer a lista inteira. Um par hostil que anuncia um milhão de itens
//!    não deve conseguir nos fazer percorrer um milhão de itens
//!    ([04, §1](../../../docs/04-seguranca.md)).
//! 3. **Caminhos depois.** Um `..` no manifesto é tentativa de escrever fora da pasta de destino,
//!    e quem recebe roda com privilégio.
//! 4. **Cota e disco por último**, porque são os únicos que dependem do estado da máquina.

use ir_proto::limits;
use ir_proto::message::{ManifestItem, RejectReason};

use crate::error::{FileError, Result};

/// Quanto este computador aceita receber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cota {
    /// Teto de bytes por transferência.
    pub bytes: u64,
    /// Teto de itens por transferência.
    pub itens: usize,
    /// Se este par pode mandar arquivo.
    ///
    /// Separada da permissão de clipboard e da de tela de bloqueio, e revogável sozinha.
    pub permitido: bool,
}

/// O teto padrão de bytes por transferência.
///
/// 16 GiB. Precisa ser confortavelmente maior que os 5 GB do critério de saída da Etapa 8 — uma
/// cota que reprovasse o próprio teste de aceitação seria uma cota errada — e finito, porque o
/// destino é quem paga o disco.
pub const BYTES_PADRAO: u64 = 16 * 1024 * 1024 * 1024;

impl Default for Cota {
    /// O padrão é **aceitar**, com teto.
    ///
    /// Ao contrário da tela de bloqueio, cujo padrão é desligado porque digitar senha na máquina
    /// do outro é um poder diferente em grau e em espécie. Copiar e colar é a função que o
    /// usuário pediu; exigir que ele a ligue depois de pareado seria cerimônia sem ganho de
    /// segurança, já que o par foi confirmado visualmente por código de seis dígitos.
    fn default() -> Self {
        Self {
            bytes: BYTES_PADRAO,
            itens: limits::MAX_MANIFEST_ITEMS,
            permitido: true,
        }
    }
}

/// O espaço livre no destino, quando a plataforma sabe dizer.
///
/// `None` significa "não sei", e não "zero": recusar por um número que não se tem seria pior que
/// tentar e falhar na escrita, que ao menos produz um erro verdadeiro.
pub type EspacoLivre = Option<u64>;

/// Decide se uma transferência é aceita, e por que não, quando não é.
///
/// Devolve `Ok(None)` para aceitar.
///
/// # Errors
///
/// [`FileError::Violacao`] quando o manifesto **não pode ser verdade** — o total declarado não
/// bate com a soma dos itens. Isso não é recusa, é o par com o codificador quebrado, e quem chama
/// derruba o enlace em vez de responder.
pub fn avaliar(
    itens: &[ManifestItem],
    total: u64,
    cota: Cota,
    livre: EspacoLivre,
) -> Result<Option<RejectReason>> {
    if !cota.permitido {
        return Ok(Some(RejectReason::NotPermitted));
    }
    if itens.len() > cota.itens || itens.len() > limits::MAX_MANIFEST_ITEMS {
        return Ok(Some(RejectReason::TooManyItems));
    }
    if itens.iter().any(|item| !item.is_safe_path()) {
        return Ok(Some(RejectReason::UnsafePath));
    }
    // A soma é conferida aqui, e não confiada: `total_bytes` é um campo do fio, e é ele que a
    // cota compara. Aceitar um total mentiroso seria aceitar a cota mentirosa junto.
    let soma = itens
        .iter()
        .filter(|item| !item.is_dir)
        .try_fold(0u64, |acc, item| acc.checked_add(item.size));
    match soma {
        Some(soma) if soma == total => {}
        _ => {
            return Err(FileError::Violacao(
                "o total do manifesto não bate com os itens",
            ));
        }
    }
    if total > cota.bytes {
        return Ok(Some(RejectReason::OverQuota));
    }
    if livre.is_some_and(|livre| total > livre) {
        return Ok(Some(RejectReason::NoDiskSpace));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arquivo(caminho: &str, tamanho: u64) -> ManifestItem {
        ManifestItem {
            path: caminho.to_owned(),
            size: tamanho,
            is_dir: false,
        }
    }

    fn pasta(caminho: &str) -> ManifestItem {
        ManifestItem {
            path: caminho.to_owned(),
            size: 0,
            is_dir: true,
        }
    }

    /// Um manifesto comum: uma pasta e dois arquivos, somando 300 bytes.
    fn comum() -> (Vec<ManifestItem>, u64) {
        (
            vec![
                pasta("relatorio"),
                arquivo("relatorio/a.pdf", 100),
                arquivo("relatorio/b.bin", 200),
            ],
            300,
        )
    }

    #[test]
    fn uma_transferencia_comum_e_aceita() {
        let (itens, total) = comum();
        assert_eq!(
            avaliar(&itens, total, Cota::default(), Some(1 << 40)).unwrap(),
            None
        );
    }

    #[test]
    fn a_cota_padrao_aceita_os_cinco_gigabytes_do_criterio_de_saida() {
        // Uma cota que reprovasse o teste de aceitação da própria etapa seria uma cota errada.
        let cinco_gb = 5 * 1000 * 1000 * 1000;
        let itens = vec![arquivo("imagem.iso", cinco_gb)];
        assert_eq!(
            avaliar(&itens, cinco_gb, Cota::default(), Some(u64::MAX)).unwrap(),
            None
        );
    }

    #[test]
    fn sem_permissao_a_resposta_nao_depende_de_mais_nada() {
        // Nem do tamanho, nem dos caminhos, nem do disco: o par não autorizado recebe sempre a
        // mesma recusa, e nada sobre a configuração desta máquina vaza na resposta.
        let cota = Cota {
            permitido: false,
            ..Cota::default()
        };
        let enormes = vec![arquivo("../fuga.txt", u64::MAX)];
        assert_eq!(
            avaliar(&enormes, u64::MAX, cota, Some(0)).unwrap(),
            Some(RejectReason::NotPermitted)
        );
    }

    #[test]
    fn o_numero_de_itens_e_conferido_antes_dos_caminhos() {
        // A ordem é a defesa: um par que anuncia itens demais não deve nos fazer percorrer a
        // lista. Se os caminhos fossem conferidos primeiro, a resposta aqui seria `UnsafePath`.
        let demais = vec![arquivo("../fuga.txt", 0); limits::MAX_MANIFEST_ITEMS + 1];
        assert_eq!(
            avaliar(&demais, 0, Cota::default(), None).unwrap(),
            Some(RejectReason::TooManyItems)
        );
    }

    #[test]
    fn um_caminho_de_fuga_e_recusado() {
        for fuga in ["../fora.txt", "/etc/passwd", "a/../../fora", "C:/Windows/x"] {
            let itens = vec![arquivo(fuga, 10)];
            assert_eq!(
                avaliar(&itens, 10, Cota::default(), None).unwrap(),
                Some(RejectReason::UnsafePath),
                "{fuga} deveria ser recusado"
            );
        }
    }

    #[test]
    fn passar_da_cota_e_recusa_com_motivo() {
        let cota = Cota {
            bytes: 100,
            ..Cota::default()
        };
        let itens = vec![arquivo("grande.bin", 101)];
        assert_eq!(
            avaliar(&itens, 101, cota, None).unwrap(),
            Some(RejectReason::OverQuota)
        );
    }

    #[test]
    fn a_cota_e_exata_e_nao_aproximada() {
        let cota = Cota {
            bytes: 100,
            ..Cota::default()
        };
        let no_limite = vec![arquivo("x.bin", 100)];
        assert_eq!(avaliar(&no_limite, 100, cota, None).unwrap(), None);
    }

    #[test]
    fn sem_disco_e_recusa_e_nao_uma_escrita_que_falha_no_meio() {
        let itens = vec![arquivo("x.bin", 1000)];
        assert_eq!(
            avaliar(&itens, 1000, Cota::default(), Some(999)).unwrap(),
            Some(RejectReason::NoDiskSpace)
        );
    }

    #[test]
    fn nao_saber_o_espaco_livre_nao_e_o_mesmo_que_nao_ter() {
        // `None` é "não sei". Recusar por um número que não se tem seria pior que tentar e falhar
        // na escrita, que ao menos produz um erro verdadeiro.
        let itens = vec![arquivo("x.bin", 1000)];
        assert_eq!(avaliar(&itens, 1000, Cota::default(), None).unwrap(), None);
    }

    #[test]
    fn um_total_mentiroso_derruba_o_enlace_em_vez_de_virar_recusa() {
        // Não há `RejectReason` para isto, e é de propósito: um manifesto cujo total não bate com
        // os itens significa que o codificador do par está quebrado. Não há resposta cortês a dar.
        let (itens, _) = comum();
        let erro = avaliar(&itens, 999_999, Cota::default(), None).unwrap_err();
        assert!(erro.derruba_o_enlace(), "{erro}");
    }

    #[test]
    fn um_total_que_estoura_a_soma_nao_causa_panico() {
        let itens = vec![arquivo("a", u64::MAX), arquivo("b", 2)];
        let erro = avaliar(&itens, 1, Cota::default(), None).unwrap_err();
        assert!(erro.derruba_o_enlace());
    }

    #[test]
    fn diretorio_nao_conta_para_o_total() {
        let itens = vec![pasta("a"), pasta("a/b")];
        assert_eq!(avaliar(&itens, 0, Cota::default(), None).unwrap(), None);
    }
}
