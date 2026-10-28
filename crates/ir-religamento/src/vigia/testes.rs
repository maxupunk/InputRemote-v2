use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// Um rádio de mentira: conta os religamentos e responde o que o teste mandar.
struct Falso {
    sabe: bool,
    aparelhos: Result<usize, String>,
    falha: Option<String>,
    religou: AtomicUsize,
}

impl Falso {
    fn base() -> Self {
        Self {
            sabe: true,
            aparelhos: Ok(0),
            falha: None,
            religou: AtomicUsize::new(0),
        }
    }

    fn novo() -> Arc<Self> {
        Arc::new(Self::base())
    }

    fn vezes(&self) -> usize {
        self.religou.load(Ordering::SeqCst)
    }
}

impl Adaptador for Falso {
    fn sabe_religar(&self) -> bool {
        self.sabe
    }

    fn aparelhos_conectados(&self) -> Result<usize, String> {
        self.aparelhos.clone()
    }

    fn religar(&self) -> Result<(), String> {
        self.religou.fetch_add(1, Ordering::SeqCst);
        self.falha.clone().map_or(Ok(()), Err)
    }
}

const SO_PELA_REDE: Enlaces = Enlaces {
    radio_esperado: true,
    radio: false,
    rede: true,
};

const OS_DOIS: Enlaces = Enlaces {
    radio_esperado: true,
    radio: true,
    rede: true,
};

const NENHUM: Enlaces = Enlaces {
    radio_esperado: true,
    radio: false,
    rede: false,
};

/// Vigia até o religamento em curso terminar — ele roda numa thread.
fn esperar(vigia: &mut Vigia, enlaces: Enlaces, agora: Instant) {
    for _ in 0..200 {
        vigia.vigiar(enlaces, agora);
        if vigia.em_curso.is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("o religamento não terminou");
}

#[test]
fn com_os_dois_enlaces_de_pe_nao_ha_aviso() {
    let falso = Falso::novo();
    let mut vigia = Vigia::new(falso.clone());
    let t0 = Instant::now();
    assert!(!vigia.vigiar(OS_DOIS, t0));
    assert!(!vigia.vigiar(OS_DOIS, t0 + PRAZO * 10));
    assert_eq!(vigia.situacao(), None);
    assert_eq!(falso.vezes(), 0);
}

#[test]
fn o_soluco_que_volta_antes_do_prazo_nao_religa() {
    let falso = Falso::novo();
    let mut vigia = Vigia::new(falso.clone());
    let t0 = Instant::now();
    vigia.vigiar(SO_PELA_REDE, t0);
    vigia.vigiar(
        SO_PELA_REDE,
        t0 + PRAZO.saturating_sub(Duration::from_secs(1)),
    );
    vigia.vigiar(OS_DOIS, t0 + PRAZO);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO + Duration::from_secs(1));
    assert_eq!(vigia.situacao(), None);
    assert_eq!(falso.vezes(), 0);
}

#[test]
fn o_radio_travado_com_a_rede_de_pe_e_religado_sozinho_e_o_aviso_some_quando_volta() {
    // O caso do log 76: o Bluetooth fora há um minuto, e a rede provando que o par está lá.
    let falso = Falso::novo();
    let mut vigia = Vigia::new(falso.clone());
    let t0 = Instant::now();
    vigia.vigiar(SO_PELA_REDE, t0);
    assert!(vigia.vigiar(SO_PELA_REDE, t0 + PRAZO), "o aviso aparece");
    assert_eq!(vigia.situacao(), Some(BluetoothParado::Religando));
    esperar(&mut vigia, SO_PELA_REDE, t0 + PRAZO);
    assert_eq!(falso.vezes(), 1);
    assert_eq!(vigia.situacao(), Some(BluetoothParado::Religando));

    assert!(vigia.vigiar(OS_DOIS, t0 + PRAZO * 2), "o aviso some");
    assert_eq!(vigia.situacao(), None);
}

#[test]
fn religa_uma_vez_so_por_queda_e_depois_diz_que_nao_resolveu() {
    let falso = Falso::novo();
    let mut vigia = Vigia::new(falso.clone());
    let t0 = Instant::now();
    vigia.vigiar(SO_PELA_REDE, t0);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO);
    esperar(&mut vigia, SO_PELA_REDE, t0 + PRAZO);

    // O prazo conta de novo a partir do religamento, para a rediscagem ter a vez dela.
    vigia.vigiar(
        SO_PELA_REDE,
        t0 + (PRAZO * 2).saturating_sub(Duration::from_secs(1)),
    );
    assert_eq!(vigia.situacao(), Some(BluetoothParado::Religando));
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO * 2);
    assert_eq!(vigia.situacao(), Some(BluetoothParado::NaoResolveu));
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO * 20);
    assert_eq!(falso.vezes(), 1, "uma vez só");

    // Quando o Bluetooth volta, a próxima queda pode religar de novo.
    vigia.vigiar(OS_DOIS, t0 + PRAZO * 21);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO * 22);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO * 23);
    esperar(&mut vigia, SO_PELA_REDE, t0 + PRAZO * 23);
    assert_eq!(falso.vezes(), 2);
}

#[test]
fn com_outro_aparelho_conectado_nao_religa_sozinho_mas_o_botao_religa() {
    let falso = Arc::new(Falso {
        aparelhos: Ok(1),
        ..Falso::base()
    });
    let mut vigia = Vigia::new(falso.clone());
    let t0 = Instant::now();
    vigia.vigiar(SO_PELA_REDE, t0);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO);
    esperar(&mut vigia, SO_PELA_REDE, t0 + PRAZO);
    assert_eq!(falso.vezes(), 0, "o fone da pessoa não cai sem ela pedir");
    assert_eq!(vigia.situacao(), Some(BluetoothParado::ComOutrosAparelhos));
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO * 5);
    assert_eq!(falso.vezes(), 0);

    assert_eq!(vigia.religar_agora(), Resposta::Feito);
    assert!(
        vigia.vigiar(SO_PELA_REDE, t0 + PRAZO * 5),
        "a janela vê o religando"
    );
    esperar(&mut vigia, SO_PELA_REDE, t0 + PRAZO * 5);
    assert_eq!(falso.vezes(), 1);
}

#[test]
fn sem_saber_quem_mais_esta_no_radio_nao_religa_sozinho() {
    let falso = Arc::new(Falso {
        aparelhos: Err("bluetoothctl ausente".to_owned()),
        ..Falso::base()
    });
    let mut vigia = Vigia::new(falso.clone());
    let t0 = Instant::now();
    vigia.vigiar(SO_PELA_REDE, t0);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO);
    esperar(&mut vigia, SO_PELA_REDE, t0 + PRAZO);
    assert_eq!(falso.vezes(), 0);
    assert_eq!(vigia.situacao(), Some(BluetoothParado::ComOutrosAparelhos));
}

#[test]
fn sem_rede_nao_ha_prova_de_que_o_par_esta_la_e_nada_se_religa() {
    let falso = Falso::novo();
    let mut vigia = Vigia::new(falso.clone());
    let t0 = Instant::now();
    vigia.vigiar(NENHUM, t0);
    vigia.vigiar(NENHUM, t0 + PRAZO * 10);
    let fixado_na_rede = Enlaces {
        radio_esperado: false,
        ..SO_PELA_REDE
    };
    vigia.vigiar(fixado_na_rede, t0 + PRAZO * 11);
    vigia.vigiar(fixado_na_rede, t0 + PRAZO * 20);
    assert_eq!(vigia.situacao(), None);
    assert_eq!(falso.vezes(), 0);
}

#[test]
fn onde_nao_se_sabe_religar_o_aviso_ensina_o_caminho_do_sistema() {
    let falso = Arc::new(Falso {
        sabe: false,
        ..Falso::base()
    });
    let mut vigia = Vigia::new(falso.clone());
    let t0 = Instant::now();
    vigia.vigiar(SO_PELA_REDE, t0);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO);
    assert_eq!(vigia.situacao(), Some(BluetoothParado::SemReligar));
    assert_eq!(vigia.religar_agora(), Resposta::Falha(Falha::SemBluetooth));

    // Uma queda da rede no meio não transforma o aviso em "religado e não resolveu".
    vigia.vigiar(NENHUM, t0 + PRAZO * 2);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO * 3);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO * 4);
    assert_eq!(vigia.situacao(), Some(BluetoothParado::SemReligar));
    assert_eq!(falso.vezes(), 0);
}

#[test]
fn o_sistema_que_recusa_vira_o_caminho_do_sistema() {
    let falso = Arc::new(Falso {
        falha: Some("org.bluez.Error.Busy".to_owned()),
        ..Falso::base()
    });
    let mut vigia = Vigia::new(falso.clone());
    let t0 = Instant::now();
    vigia.vigiar(SO_PELA_REDE, t0);
    vigia.vigiar(SO_PELA_REDE, t0 + PRAZO);
    esperar(&mut vigia, SO_PELA_REDE, t0 + PRAZO);
    assert_eq!(vigia.situacao(), Some(BluetoothParado::SemReligar));
}

#[test]
fn a_mudanca_e_contada_uma_vez_so() {
    let mut vigia = Vigia::new(Falso::novo());
    let t0 = Instant::now();
    vigia.vigiar(SO_PELA_REDE, t0);
    assert!(vigia.vigiar(SO_PELA_REDE, t0 + PRAZO));
    assert!(!vigia.vigiar(SO_PELA_REDE, t0 + PRAZO));
}
