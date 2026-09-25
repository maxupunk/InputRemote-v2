#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

fn pasta(rotulo: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ir-config-{rotulo}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_identidade_gerada_volta_igual_na_proxima_subida() {
    let dir = pasta("identidade");
    let primeira = load_identity(&dir).unwrap();
    let segunda = load_identity(&dir).unwrap();
    assert_eq!(primeira.public(), segunda.public());
    assert!(!dir.join("identity.key.tmp").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_chave_nasce_so_do_dono_e_uma_aberta_e_fechada() {
    use std::os::unix::fs::PermissionsExt;
    let dir = pasta("modo");
    let _ = load_identity(&dir).unwrap();
    let chave = dir.join("identity.key");
    let modo = |c: &Path| std::fs::metadata(c).unwrap().permissions().mode() & 0o777;
    assert_eq!(modo(&chave), 0o600);

    std::fs::set_permissions(&chave, std::fs::Permissions::from_mode(0o644)).unwrap();
    let _ = load_identity(&dir).unwrap();
    assert_eq!(
        modo(&chave),
        0o600,
        "uma chave aberta por versão antiga é fechada"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn um_arquivo_de_antes_do_controle_simetrico_sobe_com_os_dois_controlando() {
    let antigo = "role = \"client\"\npeer_edge = \"left\"\nport = 52525\n\
                  screen_width = 1920\nscreen_height = 1080\npeers = []\n";
    let config: Config = toml::from_str(antigo).unwrap();
    assert_eq!(config.policy().unwrap(), Policy::Both);
}

#[test]
fn a_tela_de_bloqueio_vem_permitida_mesmo_num_arquivo_de_antes() {
    // Log 53: antes gravava-se a permissão, desligada por padrão. O campo velho é ignorado, e o
    // par pareado passa a poder — os dois controlam um ao outro também na tela de bloqueio.
    let antigo = "pubkey = \"00\"
addr = \"10.0.0.2:52525\"
tela_de_bloqueio = false
";
    let par: PinnedPeer = toml::from_str(antigo).unwrap();
    assert!(par.permite_tela_de_bloqueio());

    let novo = toml::to_string(&par).unwrap();
    assert!(
        !novo.contains("tela_de_bloqueio"),
        "o padrão não vai ao arquivo: {novo}"
    );

    let recusando = PinnedPeer {
        recusa_tela_de_bloqueio: true,
        ..par
    };
    let texto = toml::to_string(&recusando).unwrap();
    let relido: PinnedPeer = toml::from_str(&texto).unwrap();
    assert!(
        !relido.permite_tela_de_bloqueio(),
        "a recusa escolhida fica"
    );
}

#[test]
fn a_configuracao_padrao_e_criada_e_relida() {
    let dir = pasta("config");
    let criada = load_config(&dir).unwrap();
    let relida = load_config(&dir).unwrap();
    assert_eq!(criada.politica, relida.politica);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_configuracao_gravada_nao_deixa_temporario() {
    let dir = pasta("atomica");
    let config = Config {
        port: 4321,
        ..Config::default()
    };
    config.save(&dir).unwrap();
    config.save(&dir).unwrap();
    assert!(!dir.join("config.toml.tmp").exists());
    assert_eq!(load_config(&dir).unwrap().port, 4321);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn o_endereco_configurado_vence_o_do_pareamento() {
    let mut config = Config::default();
    assert_eq!(config.endereco_do_par(), None);
    config.peers.push(PinnedPeer {
        pubkey: "00".repeat(32),
        addr: Some("10.0.0.2:52525".to_owned()),
        radio: None,
        nome: None,
        recusa_tela_de_bloqueio: false,
    });
    assert_eq!(config.endereco_do_par(), Some("10.0.0.2:52525"));
    config.peer_addr = Some("10.0.0.9:52525".to_owned());
    assert_eq!(config.endereco_do_par(), Some("10.0.0.9:52525"));
}

#[test]
fn o_portador_fixado_vai_ao_arquivo_e_volta() {
    let mut config = Config::default();
    assert_eq!(
        config.fixado(),
        None,
        "sem nada gravado, a escolha é automática"
    );
    config.fixar(Some(Carrier::Rfcomm));
    assert_eq!(config.portador_fixado.as_deref(), Some("bluetooth"));
    assert_eq!(config.fixado(), Some(Carrier::Rfcomm));
    config.fixar(None);
    assert_eq!(config.fixado(), None);
}
