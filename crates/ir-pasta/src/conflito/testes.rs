use super::*;
use ir_proto::message::data::{is_safe_component, is_safe_relative_path};

/// 2026-10-02 14:30 em Brasília (UTC-3).
const QUANDO_NS: i64 = 1_790_962_200_000_000_000;
const BRASILIA: i32 = -3 * 3_600;

fn quando() -> Momento {
    Momento::de_unix_ns(QUANDO_NS, BRASILIA)
}

fn livre(_: &str) -> bool {
    false
}

#[test]
fn o_momento_sai_no_fuso_de_quem_le() {
    assert_eq!(
        quando(),
        Momento {
            ano: 2026,
            mes: 10,
            dia: 2,
            hora: 14,
            minuto: 30
        }
    );
    // O mesmo instante em UTC já é outra hora.
    assert_eq!(Momento::de_unix_ns(QUANDO_NS, 0).hora, 17);
}

#[test]
fn o_calendario_acerta_as_viradas() {
    let dia = |segundos: i64| Momento::de_unix_ns(segundos * 1_000_000_000, 0);
    let ymd = |m: Momento| (m.ano, m.mes, m.dia);
    assert_eq!(ymd(dia(0)), (1970, 1, 1));
    assert_eq!(ymd(dia(-1)), (1969, 12, 31), "antes de 1970 também");
    assert_eq!(ymd(dia(951_782_400)), (2000, 2, 29), "bissexto de 400 anos");
    assert_eq!(ymd(dia(1_709_164_800)), (2024, 2, 29));
    assert_eq!(ymd(dia(1_709_251_200)), (2024, 3, 1));
    assert_eq!(ymd(dia(4_107_456_000)), (2100, 2, 28));
    assert_eq!(ymd(dia(4_107_542_400)), (2100, 3, 1), "2100 não é bissexto");
}

#[test]
fn a_copia_de_conflito_fica_ao_lado_com_maquina_e_hora() {
    assert_eq!(
        nome_de_conflito("relatório/março.xlsx", "NOTEBOOK", quando(), livre),
        "relatório/março (conflito NOTEBOOK 2026-10-02 14h30).xlsx"
    );
    assert_eq!(
        nome_de_conflito("notas.txt", "PC da sala", quando(), livre),
        "notas (conflito PC da sala 2026-10-02 14h30).txt"
    );
}

#[test]
fn sem_extensao_ou_com_ponto_no_comeco_a_etiqueta_vai_no_fim() {
    assert_eq!(
        nome_de_conflito("Makefile", "PC", quando(), livre),
        "Makefile (conflito PC 2026-10-02 14h30)"
    );
    assert_eq!(
        nome_de_conflito("casa/.bashrc", "PC", quando(), livre),
        "casa/.bashrc (conflito PC 2026-10-02 14h30)"
    );
    // Só a última extensão fica no fim: é ela que diz qual programa abre o arquivo.
    assert_eq!(
        nome_de_conflito("backup.tar.gz", "PC", quando(), livre),
        "backup.tar (conflito PC 2026-10-02 14h30).gz"
    );
}

#[test]
fn um_nome_ocupado_ganha_numero() {
    let ocupados = [
        "a (conflito PC 2026-10-02 14h30).txt",
        "a (conflito PC 2026-10-02 14h30 2).txt",
    ];
    let existe = |c: &str| ocupados.contains(&c);
    assert_eq!(
        nome_de_conflito("a.txt", "PC", quando(), existe),
        "a (conflito PC 2026-10-02 14h30 3).txt"
    );
}

#[test]
fn um_nome_no_limite_encurta_o_radical_e_nunca_a_etiqueta() {
    // Radical de 250 bytes em caracteres de dois bytes: o corte não pode partir um caractere.
    let radical = "ç".repeat(125);
    let caminho = format!("pasta/{radical}.docx");
    let conflito = nome_de_conflito(&caminho, "NOTEBOOK", quando(), livre);
    let componente = conflito.rsplit_once('/').unwrap().1;
    assert!(componente.len() <= 255, "{} bytes", componente.len());
    assert!(componente.ends_with(" (conflito NOTEBOOK 2026-10-02 14h30).docx"));
    assert!(componente.starts_with("çç"));
    assert!(is_safe_relative_path(&conflito));
}

#[test]
fn o_nome_que_o_par_diz_de_si_nao_vira_caminho() {
    let conflito = nome_de_conflito("a.txt", "../C:\\evil*", quando(), livre);
    assert!(is_safe_relative_path(&conflito), "{conflito}");
    assert!(!conflito.contains('/'), "{conflito}");
    assert_eq!(
        nome_de_conflito("a.txt", " .. ", quando(), livre),
        "a (conflito outro computador 2026-10-02 14h30).txt"
    );
    let longo = "M".repeat(200);
    let conflito = nome_de_conflito("a.txt", &longo, quando(), livre);
    assert!(
        conflito.len() < 100,
        "o nome da máquina é cortado: {conflito}"
    );
}

#[test]
fn todo_nome_gerado_e_um_componente_valido_nos_dois_sistemas() {
    let casos = ["a.txt", "x", ".oculto", "pasta/sub/arq.final.pdf", "ç.ã"];
    for caso in casos {
        let conflito = nome_de_conflito(caso, "Máquina 1", quando(), livre);
        let componente = conflito
            .rsplit_once('/')
            .map_or(conflito.as_str(), |(_, n)| n);
        assert!(is_safe_component(componente), "{conflito}");
    }
}

#[test]
fn a_versao_mais_recente_fica_com_o_nome_no_relogio_da_origem() {
    assert_eq!(quem_fica_com_o_nome(100, 200, 0), Lado::Replica);
    assert_eq!(quem_fica_com_o_nome(200, 100, 0), Lado::Origem);
    assert_eq!(
        quem_fica_com_o_nome(100, 100, 0),
        Lado::Origem,
        "empate é da origem"
    );
}

#[test]
fn um_relogio_adiantado_nao_ganha_todos_os_conflitos() {
    // A réplica está uma hora adiantada: a edição dela "às 15h" foi feita às 14h da origem, antes
    // da edição da origem às 14h30.
    let hora: i64 = 3_600 * 1_000_000_000;
    let origem = 14 * hora + hora / 2;
    let replica = 15 * hora;
    assert_eq!(quem_fica_com_o_nome(origem, replica, 0), Lado::Replica);
    assert_eq!(quem_fica_com_o_nome(origem, replica, hora), Lado::Origem);
}
