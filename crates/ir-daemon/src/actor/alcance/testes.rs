//! O alcance visto de fora: o que o serviço disca, grava e derruba para manter a rota dupla.

#![allow(clippy::expect_used)]

use ir_crypto::PublicKey;
use ir_ipc::Pedido;
use ir_proto::carrier::Carrier;
use ir_proto::ids::RadioAddress;
use ir_transporte::{Endereco, Fato};

use crate::actor::bancada::{Bancada, Feito};
use crate::config::{PinnedPeer, encode_key, load_config};

const RADIO: RadioAddress = RadioAddress([0xAC, 0x50, 0xDE, 0x47, 0xEB, 0x28]);

fn chave() -> PublicKey {
    PublicKey([9; 32])
}

fn rede() -> Endereco {
    Endereco::ler("10.0.0.135:52525").expect("endereço de rede")
}

/// Um serviço com o par gravado — pareado pela rede, sem saber ainda do rádio dele.
fn pareado_pela_rede() -> Bancada {
    let mut bancada = Bancada::nova();
    bancada.daemon.config.peers = vec![PinnedPeer {
        pubkey: encode_key(&chave()),
        addr: Some(rede().to_string()),
        radio: None,
        nome: None,
        recusa_tela_de_bloqueio: false,
    }];
    bancada.daemon.alcance.anotar(rede());
    bancada
}

fn estabeleceu(bancada: &mut Bancada, portador: Carrier, de: Endereco) {
    bancada.daemon.on_fato_do_transporte(Fato::Estabelecido {
        portador,
        chave_do_par: chave(),
        de,
    });
}

fn discagens(feitos: &[Feito]) -> Vec<Endereco> {
    feitos
        .iter()
        .filter_map(|feito| match feito {
            Feito::Conectou { alvo, fixada: true } => Some(*alvo),
            _ => None,
        })
        .collect()
}

#[test]
fn o_radio_contado_pelo_par_e_discado_e_gravado() {
    let mut bancada = pareado_pela_rede();
    estabeleceu(&mut bancada, Carrier::Udp, rede());
    let _ = bancada.radio.feitos();

    bancada.daemon.on_radio_do_par(RADIO);

    assert_eq!(
        discagens(&bancada.radio.feitos()),
        vec![Endereco::do_radio(RADIO)],
        "com a rede de pé, o Bluetooth que faltava é discado na hora"
    );
    bancada.daemon.gravador.esperar();
    let gravada = load_config(&bancada.dir).expect("a configuração foi gravada");
    assert_eq!(
        gravada.peers.first().and_then(|par| par.radio.as_deref()),
        Some("AC:50:DE:47:EB:28"),
        "a próxima subida já sabe discar o Bluetooth"
    );
}

#[test]
fn a_rodada_disca_so_o_portador_que_falta() {
    let mut bancada = pareado_pela_rede();
    bancada.daemon.alcance.anotar(Endereco::do_radio(RADIO));
    estabeleceu(&mut bancada, Carrier::Udp, rede());
    let _ = (bancada.rede.feitos(), bancada.radio.feitos());

    bancada.daemon.reconnect_if_needed();

    assert_eq!(discagens(&bancada.radio.feitos()).len(), 1);
    assert!(
        discagens(&bancada.rede.feitos()).is_empty(),
        "a rede já está de pé; discar por cima derrubaria o enlace que funciona"
    );
}

#[test]
fn uma_discagem_sem_resposta_nao_e_repetida_na_rodada_seguinte() {
    // Discar o rádio para um par fora de alcance leva segundos; empilhar uma discagem a cada 3 s
    // enfileiraria pedidos mais depressa do que eles terminam.
    let mut bancada = pareado_pela_rede();
    bancada.daemon.alcance.anotar(Endereco::do_radio(RADIO));
    bancada.daemon.reconnect_if_needed();
    bancada.daemon.reconnect_if_needed();

    assert_eq!(discagens(&bancada.radio.feitos()).len(), 1);
}

#[test]
fn uma_discagem_que_falha_libera_a_proxima() {
    let mut bancada = pareado_pela_rede();
    bancada.daemon.alcance.anotar(Endereco::do_radio(RADIO));
    bancada.daemon.reconnect_if_needed();
    bancada.daemon.on_fato_do_transporte(Fato::Erro {
        portador: Carrier::Rfcomm,
        mensagem: "o par não respondeu".to_owned(),
    });
    bancada.daemon.reconnect_if_needed();

    assert_eq!(discagens(&bancada.radio.feitos()).len(), 2);
}

#[test]
fn com_o_bluetooth_fixado_a_rede_nao_e_discada() {
    let mut bancada = pareado_pela_rede();
    bancada.daemon.alcance.anotar(Endereco::do_radio(RADIO));
    bancada.daemon.config.fixar(Some(Carrier::Rfcomm));

    bancada.daemon.reconnect_if_needed();

    assert_eq!(discagens(&bancada.radio.feitos()).len(), 1);
    assert!(discagens(&bancada.rede.feitos()).is_empty());
}

#[test]
fn a_queda_de_um_portador_nao_derruba_o_outro() {
    let mut bancada = pareado_pela_rede();
    estabeleceu(&mut bancada, Carrier::Udp, rede());
    estabeleceu(&mut bancada, Carrier::Rfcomm, Endereco::do_radio(RADIO));

    bancada.daemon.on_fato_do_transporte(Fato::Caiu {
        portador: Carrier::Rfcomm,
        motivo: "o par encerrou o canal".to_owned(),
    });

    assert!(bancada.daemon.linked(), "a rede continua de pé");
    assert!(bancada.daemon.alcance.de_pe(Carrier::Udp));
    assert!(!bancada.daemon.alcance.de_pe(Carrier::Rfcomm));
}

#[test]
fn encerrar_derruba_os_dois_enlaces() {
    let mut bancada = pareado_pela_rede();
    estabeleceu(&mut bancada, Carrier::Udp, rede());
    estabeleceu(&mut bancada, Carrier::Rfcomm, Endereco::do_radio(RADIO));
    let _ = (bancada.rede.feitos(), bancada.radio.feitos());

    let _ = bancada
        .daemon
        .tratar(Pedido::Encerrar, ir_transferencia::Leitor::Proprio);

    assert!(bancada.rede.feitos().contains(&Feito::Desconectou));
    assert!(bancada.radio.feitos().contains(&Feito::Desconectou));
    assert!(!bancada.daemon.linked());
}

#[test]
fn outro_computador_num_portador_nao_entra_na_rota() {
    // A garantia que torna a rota dupla segura: os dois enlaces precisam ser com a mesma chave
    // fixada. Um par de outra identidade pelo rádio é recusado, e a rota fica só com a rede.
    let mut bancada = pareado_pela_rede();
    estabeleceu(&mut bancada, Carrier::Udp, rede());
    let _ = bancada.radio.feitos();

    bancada.daemon.on_fato_do_transporte(Fato::Estabelecido {
        portador: Carrier::Rfcomm,
        chave_do_par: PublicKey([1; 32]),
        de: Endereco::do_radio(RADIO),
    });

    assert!(!bancada.daemon.alcance.de_pe(Carrier::Rfcomm));
    assert!(bancada.radio.feitos().contains(&Feito::Desconectou));
}

#[test]
fn a_configuracao_antiga_com_endereco_de_radio_vale_como_radio() {
    // Arquivos gravados antes da rota dupla têm só `addr`, e ele pode ser de rádio.
    let mut bancada = Bancada::nova();
    bancada.daemon.config.peers = vec![PinnedPeer {
        pubkey: encode_key(&chave()),
        addr: Some("AC:50:DE:47:EB:28".to_owned()),
        radio: None,
        nome: None,
        recusa_tela_de_bloqueio: false,
    }];
    let alcance = super::da_configuracao(&bancada.daemon.config);
    assert_eq!(
        alcance.endereco(Carrier::Rfcomm),
        Some(Endereco::do_radio(RADIO))
    );
    assert_eq!(alcance.endereco(Carrier::Udp), None);
}

/// Um serviço sem par gravado, com o código de um pareamento pela rede na tela.
fn pareando_pela_rede() -> Bancada {
    let mut bancada = Bancada::nova();
    bancada
        .daemon
        .on_fato_do_transporte(Fato::CodigoDePareamento {
            portador: Carrier::Udp,
            digitos: [1, 2, 3, 4, 5, 6],
            chave_do_par: chave(),
            de: rede(),
        });
    assert!(bancada.daemon.pareando());
    bancada
}

#[test]
fn um_enlace_pelo_outro_portador_nao_conclui_o_pareamento_nem_o_desfaz() {
    // Com a rota dupla, o outro lado pode discar o rádio por conta própria enquanto o código ainda
    // está na tela. Aceitar gravaria a chave sem a confirmação do usuário (docs/04 §3.2).
    let mut bancada = pareando_pela_rede();
    estabeleceu(&mut bancada, Carrier::Rfcomm, Endereco::do_radio(RADIO));

    assert!(bancada.daemon.config.peers.is_empty(), "nada foi gravado");
    assert!(!bancada.daemon.alcance.de_pe(Carrier::Rfcomm));
    assert!(bancada.radio.feitos().contains(&Feito::Desconectou));

    bancada.daemon.on_fato_do_transporte(Fato::Caiu {
        portador: Carrier::Rfcomm,
        motivo: "pedido local".to_owned(),
    });
    assert!(
        bancada.daemon.pareando(),
        "a queda do enlace recusado não desfaz o pareamento pela rede"
    );
}

#[test]
fn outra_chave_pelo_portador_do_pareamento_nao_o_conclui() {
    let mut bancada = pareando_pela_rede();
    bancada.daemon.on_fato_do_transporte(Fato::Estabelecido {
        portador: Carrier::Udp,
        chave_do_par: PublicKey([1; 32]),
        de: rede(),
    });
    assert!(bancada.daemon.config.peers.is_empty());
    assert!(!bancada.daemon.linked());
}

#[test]
fn o_enlace_do_pareamento_conclui_e_grava_a_chave() {
    let mut bancada = pareando_pela_rede();
    estabeleceu(&mut bancada, Carrier::Udp, rede());

    assert!(!bancada.daemon.pareando());
    assert_eq!(bancada.daemon.config.first_peer_key(), Some(chave()));
    assert!(bancada.daemon.alcance.de_pe(Carrier::Udp));
}
