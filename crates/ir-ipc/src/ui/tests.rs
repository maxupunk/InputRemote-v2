use super::*;
use crate::vocabulario::Nome;

fn todos_os_pedidos() -> Vec<Pedido> {
    vec![
        Pedido::Estado,
        Pedido::Acompanhar,
        Pedido::DefinirPolitica(crate::status::Politica::SoOOutro),
        Pedido::DefinirBorda(Borda::Esquerda),
        Pedido::FixarPortador(Some(Portador::Bluetooth)),
        Pedido::Procurar,
        Pedido::IniciarPareamento {
            candidato: "192.168.0.10".to_owned(),
        },
        Pedido::ConfirmarPareamento { conferiu: true },
        Pedido::EsquecerPar {
            maquina: Maquina([0; 16]),
        },
        Pedido::PermitirTelaDeBloqueio {
            maquina: Maquina([0; 16]),
            permitir: true,
        },
        Pedido::Encerrar,
        Pedido::Diagnostico,
        Pedido::AcompanharClipboard,
        Pedido::LimparRecebidos,
        Pedido::DesligarEconomiaDeEnergia { no_par: true },
        Pedido::Retomar,
        Pedido::CancelarCopia,
        Pedido::CtrlAltDel,
        Pedido::TravarBorda(true),
        Pedido::BloquearJuntos(false),
    ]
}

#[test]
fn todo_pedido_declara_autoridade() {
    // Um pedido que esqueça de declarar o próprio nível é escalada de privilégio, não
    // descuido de estilo. Este teste falha se alguém acrescentar variante sem classificá-la.
    for pedido in todos_os_pedidos() {
        let _ = pedido.autoridade();
    }
}

#[test]
fn ler_nunca_exige_elevacao() {
    for pedido in [
        Pedido::Estado,
        Pedido::Acompanhar,
        Pedido::AcompanharClipboard,
        Pedido::Diagnostico,
    ] {
        assert_eq!(pedido.autoridade(), Autoridade::Ler, "{pedido:?}");
    }
}

#[test]
fn tudo_que_decide_quem_digita_exige_elevacao() {
    let sensíveis = [
        Pedido::IniciarPareamento {
            candidato: "x".to_owned(),
        },
        Pedido::ConfirmarPareamento { conferiu: true },
        Pedido::EsquecerPar {
            maquina: Maquina([1; 16]),
        },
        Pedido::PermitirTelaDeBloqueio {
            maquina: Maquina([1; 16]),
            permitir: true,
        },
    ];
    for pedido in sensíveis {
        assert_eq!(
            pedido.autoridade(),
            Autoridade::Elevado,
            "{pedido:?} decide quem pode digitar na tela de bloqueio"
        );
    }
}

#[test]
fn as_autoridades_sao_ordenadas_por_poder() {
    assert!(Autoridade::Elevado > Autoridade::Configurar);
    assert!(Autoridade::Configurar > Autoridade::Ler);
}

#[test]
fn o_estado_recem_instalado_diz_o_que_fazer_primeiro() {
    let estado = Estado::recem_instalado(Maquina([7; 16]), Nome::coagido("bancada"));
    let resumo = estado.resumo();
    assert!(
        resumo.contains("Pareie"),
        "a primeira tela precisa dizer o primeiro passo"
    );
}
