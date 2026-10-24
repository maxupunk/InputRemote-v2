use super::*;
use ir_ipc::{AvisoDoPainel, ComandoDePainel, Falha, Resposta};
use ir_monitor::Situacao;
use ir_proto::message::UsageSample;
use ir_session::Millis;
use tokio::sync::broadcast;

/// Uma sessão falsa: com ou sem par, e se ele entende a versão 11.
struct Falsa(Option<(&'static str, bool)>);

impl Sessao for Falsa {
    fn par(&self) -> Option<(&str, bool)> {
        self.0
    }
}

/// A vigilância de teste, o que ela avisa e onde grava.
struct Bancada {
    vigilancia: Vigilancia,
    avisos: broadcast::Receiver<Aviso>,
    config: Config,
    gravador: Gravador,
    pasta: std::path::PathBuf,
}

impl Drop for Bancada {
    /// O teste não deixa a configuração gravada para trás.
    fn drop(&mut self) {
        self.gravador.esperar();
        let _ = std::fs::remove_dir_all(&self.pasta);
    }
}

impl Bancada {
    /// A entrega devolve `false`: uma coleta que comece para na primeira amostra.
    fn nova(nome: &str) -> Self {
        let (avisos, recebidos) = broadcast::channel(16);
        let pasta =
            std::env::temp_dir().join(format!("ir-vigilancia-{nome}-{}", std::process::id()));
        Self {
            vigilancia: Vigilancia::nova("DESKTOP", Identity::generate(), avisos, |_| false),
            avisos: recebidos,
            config: Config::default(),
            gravador: Gravador::novo(pasta.clone()),
            pasta,
        }
    }

    fn evento(&mut self, evento: Evento) -> Vec<Input> {
        self.vigilancia
            .evento(evento, &mut self.config, &self.gravador)
    }

    fn assinantes(&mut self, quantos: usize) {
        let _ = self.evento(Evento::Monitor(ir_monitor_rede::Aviso::Assinantes(quantos)));
    }

    fn avisos(&mut self) -> Vec<Aviso> {
        std::iter::from_fn(|| self.avisos.try_recv().ok()).collect()
    }
}

fn pedidos(entradas: &[Input]) -> Vec<bool> {
    entradas
        .iter()
        .filter_map(|e| match e {
            Input::WatchPeerUsage(quer) => Some(*quer),
            _ => None,
        })
        .collect()
}

#[test]
fn sem_assinante_nada_e_coletado_nem_pedido_ao_par() {
    let mut bancada = Bancada::nova("sem-assinante");
    let entradas = bancada
        .vigilancia
        .batida(&Falsa(Some(("nb", true))), &bancada.config);
    assert!(pedidos(&entradas).is_empty());
    assert!(!bancada.vigilancia.coletando());
}

#[test]
fn com_assinante_pede_ao_par_renova_e_desfaz_ao_sair() {
    let mut bancada = Bancada::nova("assinante");
    let sessao = Falsa(Some(("nb", true)));
    bancada.assinantes(1);
    assert!(bancada.vigilancia.coletando());

    let primeiras: Vec<bool> = (0..4)
        .flat_map(|_| pedidos(&bancada.vigilancia.batida(&sessao, &bancada.config)))
        .collect();
    assert!(primeiras.iter().all(|quer| *quer));
    assert!(primeiras.len() >= 2, "o pedido é renovado");

    bancada.assinantes(0);
    assert!(
        !bancada.vigilancia.coletando(),
        "sem assinante a coleta para"
    );
    let config = bancada.config.clone();
    assert_eq!(
        pedidos(&bancada.vigilancia.batida(&sessao, &config)),
        vec![false]
    );
    assert!(
        pedidos(&bancada.vigilancia.batida(&sessao, &config)).is_empty(),
        "desfaz uma vez só"
    );
}

#[test]
fn par_antigo_nao_recebe_pedido_e_aparece_desatualizado() {
    let mut bancada = Bancada::nova("antigo");
    bancada.assinantes(1);
    let config = bancada.config.clone();
    let entradas = bancada
        .vigilancia
        .batida(&Falsa(Some(("nb", false))), &config);
    assert!(pedidos(&entradas).is_empty());
    assert_eq!(
        bancada.vigilancia.quadro().maquinas[1].situacao,
        Situacao::VersaoAntiga
    );
}

#[test]
fn o_pedido_do_par_liga_a_coleta_e_a_amostra_vai_para_ele() {
    let mut bancada = Bancada::nova("pedido-do-par");
    bancada.vigilancia.notar(&Notice::PeerWatchesUsage(true));
    assert!(bancada.vigilancia.coletando());
    let amostra = UsageSample {
        cpu_permille: Some(5),
        ..UsageSample::default()
    };
    let entradas = bancada.evento(Evento::Amostra(amostra.clone()));
    assert!(matches!(entradas.as_slice(), [Input::ShareUsage(a)] if *a == amostra));

    bancada.vigilancia.notar(&Notice::PeerWatchesUsage(false));
    assert!(!bancada.vigilancia.coletando());
    assert!(bancada.evento(Evento::Amostra(amostra)).is_empty());
}

#[test]
fn a_volta_de_cada_rota_chega_ao_quadro() {
    let mut bancada = Bancada::nova("volta");
    let config = bancada.config.clone();
    let _ = bancada
        .vigilancia
        .batida(&Falsa(Some(("nb", true))), &config);
    bancada.vigilancia.notar(&Notice::RouteLatency {
        carrier: Carrier::Rfcomm,
        rtt: Millis(7),
    });
    bancada
        .vigilancia
        .notar(&Notice::PeerUsage(UsageSample::default()));
    let quadro = bancada.vigilancia.quadro();
    assert_eq!(quadro.enlace.unwrap().bluetooth_us, Some(7_000));
    assert_eq!(
        quadro.maquinas[1].situacao,
        Situacao::Amostra(UsageSample::default())
    );
}

#[test]
fn a_queda_da_sessao_tira_o_par_do_quadro() {
    let mut bancada = Bancada::nova("queda");
    let config = bancada.config.clone();
    let _ = bancada
        .vigilancia
        .batida(&Falsa(Some(("nb", true))), &config);
    assert_eq!(bancada.vigilancia.quadro().maquinas.len(), 2);
    let _ = bancada.vigilancia.batida(&Falsa(None), &config);
    assert_eq!(bancada.vigilancia.quadro().maquinas.len(), 1);
}

#[test]
fn o_pareamento_grava_a_chave_e_avisa_a_janela() {
    let mut bancada = Bancada::nova("pareamento");
    let chave = Identity::generate().public();
    let _ = bancada.evento(Evento::Monitor(ir_monitor_rede::Aviso::Pareado(chave)));
    assert_eq!(bancada.config.chaves_dos_paineis(), vec![chave]);
    let avisos = bancada.avisos();
    assert!(avisos.contains(&Aviso::Painel(AvisoDoPainel::Concluido(true))));
    assert!(
        avisos
            .iter()
            .any(|a| matches!(a, Aviso::Painel(AvisoDoPainel::Lista(l)) if l.len() == 1))
    );

    let nome = ir_monitor_rede::Aviso::Apresentou {
        chave,
        nome: "Tablet".to_owned(),
    };
    let _ = bancada.evento(Evento::Monitor(nome));
    let resumo = bancada.vigilancia.resumo(&bancada.config);
    assert_eq!(resumo[0].nome, "Tablet");
    assert!(!resumo[0].conectado);
}

#[test]
fn sem_monitor_a_janela_ouve_que_nao_deu() {
    let mut bancada = Bancada::nova("sem-monitor");
    let resposta = bancada.vigilancia.comando(
        ComandoDePainel::Parear,
        &mut bancada.config,
        &bancada.gravador,
    );
    assert_eq!(resposta, Resposta::Falha(Falha::ForaDeContexto));
}
