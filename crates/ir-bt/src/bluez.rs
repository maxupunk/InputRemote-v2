//! O formato do armazenamento do BlueZ, lido como texto.
//!
//! O BlueZ guarda cada par conhecido em
//! `/var/lib/bluetooth/<adaptador>/<dispositivo>/info`, um arquivo no estilo INI. Ler esse
//! arquivo é como o produto descobre quem já está pareado no Linux **sem falar D-Bus** — a
//! consequência direta do [ADR-0009](../../../docs/adr/0009-canal-rfcomm-fixo-sem-sdp.md): sem
//! `ProfileManager1` não há também um `org.bluez.Device1` para consultar.
//!
//! # Por que este módulo não abre arquivo
//!
//! Ele recebe texto e devolve fatos. Nenhum caminho, nenhum `std::fs`, nenhuma permissão. Duas
//! consequências, e as duas são o ponto:
//!
//! 1. **Os testes rodam em qualquer plataforma**, inclusive no Windows desta bancada, onde o
//!    alvo Linux nem compila por falta de compilador C cruzado. A parte que mais tem como estar
//!    errada — a leitura de um formato de terceiros — é justamente a que fica verificável em
//!    todo lugar.
//! 2. **Ler o diretório exige ser root**, e isso é assunto do backend, não da análise.
//!
//! # O que conta como "pareado"
//!
//! A presença da seção `[LinkKey]`. É a chave de vínculo BR/EDR, gravada quando o pareamento do
//! sistema termina, e é ela que faz o par continuar pareado depois de reiniciar — o fato de que
//! o requisito da tela de login depende ([ADR-0005](../../../docs/adr/0005-bluetooth-rfcomm-winsock.md),
//! Consequências).
//!
//! Um dispositivo que só tem chaves de LE (`[LongTermKey]`, `[IdentityResolvingKey]`) está
//! pareado, mas **não** por BR/EDR — e RFCOMM só existe em BR/EDR. Oferecê-lo na tela seria
//! oferecer uma escolha que não pode funcionar.

/// O que o arquivo `info` de um par diz.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InfoDoPar {
    /// O nome que o sistema mostra, se houver.
    pub nome: Option<String>,
    /// Se há chave de vínculo BR/EDR — isto é, se dá para abrir um canal RFCOMM com ele.
    pub pareado_por_bredr: bool,
    /// Se o arquivo declara suporte a BR/EDR.
    ///
    /// Separado de [`Self::pareado_por_bredr`] porque são perguntas diferentes: um fone só de LE
    /// nunca vai servir, e um computador BR/EDR ainda não pareado pode passar a servir.
    pub suporta_bredr: bool,
}

/// A seção que guarda a chave de vínculo BR/EDR.
const SECAO_DA_CHAVE: &str = "LinkKey";

/// A seção dos dados gerais.
const SECAO_GERAL: &str = "General";

/// Lê o conteúdo de um arquivo `info` do BlueZ.
///
/// Tolerante de propósito: é formato de terceiros, e um campo a mais, uma seção desconhecida ou
/// uma linha estranha não podem fazer o produto deixar de enxergar um par que existe.
#[must_use]
pub fn ler_info(texto: &str) -> InfoDoPar {
    let mut info = InfoDoPar::default();
    let mut secao = String::new();
    for linha in texto.lines() {
        let linha = linha.trim();
        if linha.is_empty() || linha.starts_with('#') {
            continue;
        }
        if let Some(nome_da_secao) = secao_de(linha) {
            nome_da_secao.clone_into(&mut secao);
            if secao == SECAO_DA_CHAVE {
                info.pareado_por_bredr = true;
            }
            continue;
        }
        if secao != SECAO_GERAL {
            continue;
        }
        let Some((chave, valor)) = linha.split_once('=') else {
            continue;
        };
        match chave.trim() {
            "Name" => info.nome = nome_util(valor),
            "SupportedTechnologies" => info.suporta_bredr = valor.contains("BR/EDR"),
            _ => {}
        }
    }
    info
}

/// O nome da seção, se a linha for um cabeçalho `[Assim]`.
fn secao_de(linha: &str) -> Option<&str> {
    linha
        .strip_prefix('[')
        .and_then(|resto| resto.strip_suffix(']'))
        .map(str::trim)
}

/// O nome, se ele disser alguma coisa.
///
/// Um `Name=` vazio é pior que nome nenhum: a tela mostraria uma linha em branco para o usuário
/// escolher, e ele não teria como saber qual computador é.
fn nome_util(valor: &str) -> Option<String> {
    let limpo = valor.trim();
    (!limpo.is_empty()).then(|| limpo.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um arquivo como o BlueZ 5.87 do notebook da bancada realmente grava.
    const PAREADO: &str = "\
[General]
Name=SAMSUNG-MAXUEL
Class=0x1c010c
SupportedTechnologies=BR/EDR;
Trusted=false
Blocked=false
Services=00001105-0000-1000-8000-00805f9b34fb;

[LinkKey]
Key=0123456789ABCDEF0123456789ABCDEF
Type=4
PINLength=0

[DeviceID]
Source=1
Vendor=6
";

    #[test]
    fn um_par_de_verdade_e_lido_inteiro() {
        let info = ler_info(PAREADO);
        assert_eq!(info.nome.as_deref(), Some("SAMSUNG-MAXUEL"));
        assert!(info.pareado_por_bredr, "há [LinkKey]");
        assert!(info.suporta_bredr);
    }

    #[test]
    fn sem_chave_de_vinculo_nao_esta_pareado_por_bredr() {
        // Conhecido não é pareado. Um dispositivo visto uma vez fica no armazenamento sem
        // chave, e conectar nele daria erro — oferecê-lo na tela seria prometer o que não dá.
        let visto_so_uma_vez = "[General]\nName=Notebook\nSupportedTechnologies=BR/EDR;\n";
        let info = ler_info(visto_so_uma_vez);
        assert_eq!(info.nome.as_deref(), Some("Notebook"));
        assert!(!info.pareado_por_bredr);
    }

    #[test]
    fn um_dispositivo_so_de_le_nao_serve_para_rfcomm() {
        // Um fone ou mouse de baixa energia está pareado, e ainda assim não há RFCOMM com ele.
        let so_le = "\
[General]
Name=Fone
SupportedTechnologies=LE;

[IdentityResolvingKey]
Key=FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF

[LongTermKey]
Key=FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF
";
        let info = ler_info(so_le);
        assert!(!info.suporta_bredr, "não anuncia BR/EDR");
        assert!(
            !info.pareado_por_bredr,
            "chave de LE não é chave de vínculo"
        );
    }

    #[test]
    fn o_nome_aceita_espaco_e_acento() {
        // "Notebook do Maxuel" é o nome que o usuário reconhece. Cortar no espaço, ou perder o
        // acento, devolveria uma lista que não parece com a das configurações do sistema.
        let info = ler_info("[General]\nName=Notebook do Maxuel (Fedora)\n");
        assert_eq!(info.nome.as_deref(), Some("Notebook do Maxuel (Fedora)"));
        let com_acento = ler_info("[General]\nName=Computador da Sala — Escritório\n");
        assert_eq!(
            com_acento.nome.as_deref(),
            Some("Computador da Sala — Escritório")
        );
    }

    #[test]
    fn um_nome_vazio_vale_o_mesmo_que_nao_ter_nome() {
        // Uma linha em branco na lista não dá ao usuário como escolher.
        assert_eq!(ler_info("[General]\nName=\n").nome, None);
        assert_eq!(ler_info("[General]\nName=   \n").nome, None);
    }

    #[test]
    fn fim_de_linha_do_windows_nao_atrapalha() {
        // O arquivo é do Linux, mas o texto pode chegar aqui por outro caminho — um diagnóstico
        // colado, por exemplo. Um `\r` pendurado no fim viraria parte do nome.
        let info = ler_info("[General]\r\nName=Fedora\r\n\r\n[LinkKey]\r\nKey=00\r\n");
        assert_eq!(info.nome.as_deref(), Some("Fedora"));
        assert!(info.pareado_por_bredr);
    }

    #[test]
    fn chave_fora_da_secao_geral_nao_e_confundida_com_o_nome() {
        // `Name` também aparece em outras seções em algumas versões. Ler o de fora do
        // `[General]` daria ao par o nome errado.
        let info = ler_info("[General]\nName=Certo\n\n[DeviceID]\nName=Errado\n");
        assert_eq!(info.nome.as_deref(), Some("Certo"));
    }

    #[test]
    fn um_arquivo_estranho_nao_derruba_nada() {
        // Formato de terceiros: versão nova, campo novo, lixo. Nada disso pode virar pânico nem
        // fazer o produto deixar de enxergar o que existe.
        for estranho in [
            "",
            "sem secao nenhuma",
            "[",
            "]",
            "[]\nName=x\n",
            "[General\nName=x\n",
            "=sem chave\n",
            "[General]\n=\n",
            "[General]\nName\n",
            "#comentario\n[LinkKey]\n",
        ] {
            let _ = ler_info(estranho);
        }
        // E o caso que importa continua funcionando no meio do lixo.
        let info = ler_info("lixo\n[General]\nName=Fedora\nlixo=\n[LinkKey]\n");
        assert_eq!(info.nome.as_deref(), Some("Fedora"));
        assert!(info.pareado_por_bredr);
    }

    #[test]
    fn a_secao_da_chave_vale_mesmo_vazia() {
        // O que marca o pareamento é a seção existir. Exigir a linha `Key=` deixaria de fora
        // formatos futuros que guardem a chave de outro jeito.
        let info = ler_info("[General]\nName=x\n\n[LinkKey]\n");
        assert!(info.pareado_por_bredr);
    }
}
