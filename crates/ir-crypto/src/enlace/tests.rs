#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::{CryptoError, Handshake, Identity, PublicKey};

/// Um desenquadrador do tamanho do rádio: prefixo `u16`, teto pequeno.
type Estreito = Desenquadrador<2, 100>;
/// Um do tamanho do TCP: prefixo `u32`.
type Largo = Desenquadrador<4, 70_000>;

#[test]
fn os_bytes_de_modo_e_de_especie_sao_os_do_fio() {
    // Formato de fio: os dois lados precisam continuar se entendendo.
    assert_eq!(corpo_de_handshake(Mode::Rekey, b"m"), vec![2, b'm']);
    assert_eq!(embrulhar(Kind::PairReject, b""), vec![2]);
    for modo in [Mode::Pair, Mode::Reconnect, Mode::Rekey] {
        assert_eq!(Mode::from_byte(modo.to_byte()), Some(modo));
    }
    assert_eq!(ler_handshake(&[3, 0]), None);
    assert_eq!(desembrulhar(&[3, 0]), None);
    assert_eq!(ler_handshake(&[]), None);
}

#[test]
fn o_prefixo_e_little_endian_na_largura_pedida() {
    assert_eq!(
        enquadrar::<2, 100>(b"abc").unwrap(),
        vec![3, 0, b'a', b'b', b'c']
    );
    assert_eq!(enquadrar::<4, 100>(b"").unwrap(), vec![0, 0, 0, 0]);
}

#[test]
fn enquadrar_alem_do_teto_ou_da_largura_e_excesso() {
    assert_eq!(
        enquadrar::<2, 100>(&[0; 101]),
        Err(Excesso {
            tamanho: 101,
            limite: 100
        })
    );
    // O teto cabe, mas o prefixo de um byte não conta até 300.
    assert!(enquadrar::<1, 1000>(&[0; 300]).is_err());
}

#[test]
fn o_corpo_volta_inteiro_mesmo_picado_e_colado() {
    let mut fluxo = enquadrar::<4, 70_000>(b"um").unwrap();
    fluxo.extend(enquadrar::<4, 70_000>(b"dois").unwrap());
    let mut des = Largo::novo();
    for byte in &fluxo {
        des.alimentar(&[*byte]);
    }
    assert_eq!(des.proximo().unwrap(), Some(b"um".to_vec()));
    assert_eq!(des.proximo().unwrap(), Some(b"dois".to_vec()));
    assert_eq!(des.proximo().unwrap(), None);
    assert_eq!(des.pendentes(), 0);
}

#[test]
fn um_anuncio_acima_do_teto_e_recusado_antes_de_alocar() {
    let mut des = Estreito::novo();
    des.alimentar(&u16::MAX.to_le_bytes());
    assert_eq!(
        des.proximo(),
        Err(Excesso {
            tamanho: 65_535,
            limite: 100
        })
    );
}

#[test]
fn o_contador_implicito_so_avanca_quando_abre() {
    let mut contador = ContadorImplicito::novo();
    let falhou: Result<(), &str> = contador.abrir(|n| {
        assert_eq!(n, 1);
        Err("a tag não confere")
    });
    assert!(falhou.is_err());
    assert_eq!(contador.recebidos(), 0, "a falha não queima o número");
    let aberto: Result<u64, &str> = contador.abrir(Ok);
    assert_eq!(aberto, Ok(1), "o legítimo ainda vem com o número 1");
    assert_eq!(contador.recebidos(), 1);
}

#[test]
fn a_chave_fixada_escolhe_entre_reconectar_e_parear() {
    let chave = PublicKey([7; 32]);
    assert_eq!(
        ConnectMode::de_chave(Some(chave)),
        ConnectMode::Reconnect(chave)
    );
    assert_eq!(ConnectMode::de_chave(None), ConnectMode::Pair);
    assert_eq!(ConnectMode::Rekey(chave).modo(), Mode::Rekey);
}

/// Roda um handshake inteiro, a partir do modo, como os transportes fazem.
fn apertar(modo: ConnectMode, a: &Identity, b: &Identity) -> (Handshake, Handshake) {
    let mut ia = modo.iniciar(a).unwrap();
    let mut ib = modo.modo().responder(b).unwrap();
    while !(ia.is_finished() && ib.is_finished()) {
        if ia.is_my_turn() {
            ib.read_message(&ia.write_message().unwrap()).unwrap();
        } else {
            ia.read_message(&ib.write_message().unwrap()).unwrap();
        }
    }
    (ia, ib)
}

#[test]
fn concluir_o_pareamento_da_codigo_e_a_reconexao_nao() {
    let (a, b) = (Identity::generate(), Identity::generate());
    let (ia, ib) = apertar(ConnectMode::Pair, &a, &b);
    let (ea, eb) = (concluir(ia).unwrap(), concluir(ib).unwrap());
    assert!(ea.code.is_some());
    assert_eq!(ea.code, eb.code);
    assert_eq!(ea.peer_static, b.public());
    assert_eq!(eb.peer_static, a.public());

    for modo in [
        ConnectMode::Reconnect(b.public()),
        ConnectMode::Rekey(b.public()),
    ] {
        let (ia, ib) = apertar(modo, &a, &b);
        assert!(concluir(ia).unwrap().code.is_none());
        assert_eq!(concluir(ib).unwrap().peer_static, a.public());
    }
}

#[test]
fn concluir_um_handshake_pela_metade_e_recusado() {
    let a = Identity::generate();
    let inacabado = Handshake::pair_initiator(&a).unwrap();
    assert_eq!(concluir(inacabado).unwrap_err(), CryptoError::NotFinished);
}

#[test]
fn uma_confirmacao_so_nao_promove() {
    let mut local_primeiro = Confirmacao::default();
    assert_eq!(
        local_primeiro.local(true),
        (Kind::PairConfirm, Desfecho::Esperar)
    );
    assert_eq!(local_primeiro.do_par(), Desfecho::Promover);

    let mut par_primeiro = Confirmacao::default();
    assert_eq!(par_primeiro.do_par(), Desfecho::Esperar);
    assert_eq!(
        par_primeiro.local(true),
        (Kind::PairConfirm, Desfecho::Promover)
    );
}

#[test]
fn recusar_manda_a_recusa_e_derruba_mesmo_com_o_par_confirmado() {
    let mut confirmacao = Confirmacao::default();
    assert_eq!(confirmacao.do_par(), Desfecho::Esperar);
    assert_eq!(
        confirmacao.local(false),
        (Kind::PairReject, Desfecho::Derrubar)
    );
}
