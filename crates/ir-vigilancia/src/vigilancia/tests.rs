use super::*;
use ir_ipc::{AvisoDoPainel, ComandoDePainel, Falha, Resposta};
use ir_monitor::entre_pcs::{codificar, decodificar};
use ir_monitor::{EntrePcs, Situacao, UsoDeProcesso};
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

/// Os recados ao par, já lidos.
fn recados(entradas: &[Input]) -> Vec<EntrePcs> {
    entradas
        .iter()
        .filter_map(|e| match e {
            Input::Panel(bytes) => decodificar(bytes),
            _ => None,
        })
        .collect()
}

/// Os pedidos ao par: `true` para assinar, `false` para parar.
fn pedidos(entradas: &[Input]) -> Vec<bool> {
    recados(entradas)
        .iter()
        .filter_map(|r| match r {
            EntrePcs::Assinar(_) => Some(true),
            EntrePcs::Parar => Some(false),
            EntrePcs::Uso(..) => None,
        })
        .collect()
}

/// O recado do par, como a sessão o entrega.
fn do_par(recado: &EntrePcs) -> Notice {
    Notice::PeerPanel(codificar(recado).unwrap())
}

fn interesse(nomes: &[&str], lista: bool) -> InteresseEmProcessos {
    InteresseEmProcessos {
        nomes: nomes.iter().map(|n| (*n).to_owned()).collect(),
        lista,
    }
}

fn uso(nome: &str) -> UsoDeProcesso {
    UsoDeProcesso {
        nome: nome.to_owned(),
        instancias: 1,
        cpu_permille: 10,
        memoria_kib: 1024,
    }
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
    bancada
        .vigilancia
        .notar(&do_par(&EntrePcs::Assinar(InteresseEmProcessos::default())));
    assert!(bancada.vigilancia.coletando());
    let amostra = UsageSample {
        cpu_permille: Some(5),
        ..UsageSample::default()
    };
    let entradas = bancada.evento(Evento::Amostra(amostra.clone(), None));
    assert_eq!(
        recados(&entradas),
        vec![EntrePcs::Uso(amostra.clone(), None)]
    );

    bancada.vigilancia.notar(&do_par(&EntrePcs::Parar));
    assert!(!bancada.vigilancia.coletando());
    assert!(bancada.evento(Evento::Amostra(amostra, None)).is_empty());
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
        .notar(&do_par(&EntrePcs::Uso(UsageSample::default(), None)));
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

/// Uma medida só serve aos dois painéis: cada um recebe os processos que pediu.
#[test]
fn os_processos_vao_a_quem_pediu() {
    let mut bancada = Bancada::nova("processos");
    let [daqui, do_par_aqui] = [interesse(&["docker"], true), interesse(&["wsl"], false)];
    let _ = bancada.evento(Evento::Monitor(ir_monitor_rede::Aviso::Interesse([
        daqui,
        InteresseEmProcessos::default(),
    ])));
    bancada
        .vigilancia
        .notar(&do_par(&EntrePcs::Assinar(do_par_aqui)));
    assert_eq!(
        bancada.vigilancia.na_vigia,
        interesse(&["docker", "wsl"], true),
        "a vigia mede o que os dois querem"
    );

    let medido = Processos {
        vigiados: vec![uso("docker"), uso("wsl")],
        lista: vec![uso("chrome")],
    };
    let entradas = bancada.evento(Evento::Amostra(UsageSample::default(), Some(medido)));
    let recados = recados(&entradas);
    let [EntrePcs::Uso(_, Some(para_o_par))] = recados.as_slice() else {
        panic!("o par não recebeu a amostra com os processos")
    };
    assert_eq!(para_o_par.vigiados, vec![uso("wsl")]);
    assert!(
        para_o_par.lista.is_empty(),
        "a lista daqui não foi pedida pelo par"
    );
    let daqui = bancada.vigilancia.detalhes().processos[0].clone().unwrap();
    assert_eq!(daqui.vigiados, vec![uso("docker")]);
    assert_eq!(daqui.lista, vec![uso("chrome")]);
}

/// O painel daqui pede ao par os processos de lá; mudar o pedido vai na hora, sem esperar a
/// renovação.
#[test]
fn o_pedido_ao_par_leva_os_processos_e_muda_na_hora() {
    let mut bancada = Bancada::nova("pedido-de-processos");
    let sessao = Falsa(Some(("nb", true)));
    bancada.assinantes(1);
    let config = bancada.config.clone();
    let _ = bancada.vigilancia.batida(&sessao, &config);
    let lista_do_par = interesse(&[], true);
    let _ = bancada.evento(Evento::Monitor(ir_monitor_rede::Aviso::Interesse([
        InteresseEmProcessos::default(),
        lista_do_par.clone(),
    ])));
    let pedidos: Vec<EntrePcs> = (0..2)
        .flat_map(|_| recados(&bancada.vigilancia.batida(&sessao, &config)))
        .collect();
    assert_eq!(pedidos.first(), Some(&EntrePcs::Assinar(lista_do_par)));

    let processos = Processos {
        vigiados: Vec::new(),
        lista: vec![uso("vmmem")],
    };
    bancada.vigilancia.notar(&do_par(&EntrePcs::Uso(
        UsageSample::default(),
        Some(processos.clone()),
    )));
    assert_eq!(bancada.vigilancia.detalhes().processos[1], Some(processos));
}

#[test]
fn a_copia_chega_aos_detalhes_com_a_velocidade() {
    let mut bancada = Bancada::nova("copia");
    let copia = |feitos| {
        Evento::Copia(ir_ipc::Transferencia {
            sentido: ir_ipc::Sentido::Recebendo,
            nome: "fotos".to_owned(),
            bytes_feitos: feitos,
            bytes_total: 100,
            fase: ir_ipc::Fase::Andando,
        })
    };
    let _ = bancada.evento(copia(10));
    let primeira = bancada.vigilancia.detalhes().copia.unwrap();
    assert!(!primeira.enviando);
    assert_eq!(
        primeira.por_segundo, None,
        "uma medida só não tem velocidade"
    );
    std::thread::sleep(Duration::from_millis(20));
    let _ = bancada.evento(copia(60));
    let segunda = bancada.vigilancia.detalhes().copia.unwrap();
    assert_eq!(segunda.feitos, 60);
    assert!(segunda.por_segundo.is_some_and(|v| v > 0));
}

/// O interruptor dos pedidos vale sem o monitor de pé, fica gravado e é contado à janela.
#[test]
fn aceitar_pedidos_grava_e_avisa_a_janela() {
    let mut bancada = Bancada::nova("pedidos");
    assert!(bancada.config.painel_aceita_pedidos, "ligado por padrão");
    let resposta = bancada.vigilancia.comando(
        ComandoDePainel::AceitarPedidos(false),
        &mut bancada.config,
        &bancada.gravador,
    );
    assert_eq!(resposta, Resposta::Feito);
    assert!(!bancada.config.painel_aceita_pedidos);
    let _ = bancada.vigilancia.comando(
        ComandoDePainel::ComoEstaoOsPedidos,
        &mut bancada.config,
        &bancada.gravador,
    );
    let avisos = bancada.avisos();
    let pedidos: Vec<&Aviso> = avisos
        .iter()
        .filter(|a| matches!(a, Aviso::Painel(AvisoDoPainel::AceitaPedidos(false))))
        .collect();
    assert_eq!(pedidos.len(), 2, "{avisos:?}");
}
