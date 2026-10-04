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
        Pedido::CopiarEColar(false),
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

fn chegando() -> Aviso {
    Aviso::ArquivosChegando(crate::chegada::Chegando {
        nome: "Jogo eletronica 2".to_owned(),
        montagem: "/var/lib/inputremote/recebidos/.parcial-7".to_owned(),
        publicada_em: "/var/lib/inputremote/recebidos".to_owned(),
        itens: vec![crate::chegada::ItemChegando {
            caminho: "Jogo eletronica 2/a.png".to_owned(),
            tamanho: 3,
            pasta: false,
        }],
    })
}

#[test]
fn os_avisos_novos_vao_no_fim_e_nao_renumeram_os_antigos() {
    // O canal local é posicional, e o ajudante de clipboard sobrevive às atualizações: um número
    // que muda faz o processo de antes ler outra coisa (log 56).
    let numero = |aviso: &Aviso| postcard::to_allocvec(aviso).unwrap()[0];
    assert_eq!(numero(&Aviso::LerClipboard), 6);
    assert_eq!(numero(&Aviso::BordaAjustada(Borda::Esquerda)), 10);
    assert_eq!(numero(&chegando()), 11);
    assert_eq!(numero(&Aviso::PastasMudaram(vec![])), 12);
    assert_eq!(numero(&Aviso::RecadoDasPastas(String::new())), 13);
    assert_eq!(numero(&Aviso::ArquivosDaPasta(vec![])), 14);
    let ajustado = Aviso::CopiarEColarAjustado {
        ligado: false,
        par: String::new(),
    };
    assert_eq!(numero(&ajustado), 15);
}

#[test]
fn o_conteudo_do_clipboard_vai_so_ao_ajudante() {
    assert!(chegando().so_para_o_ajudante());
    assert!(Aviso::LerClipboard.so_para_o_ajudante());
    assert!(Aviso::ArquivosDaPasta(vec![]).so_para_o_ajudante());
    assert!(!Aviso::BordaAjustada(Borda::Esquerda).so_para_o_ajudante());
}

#[test]
fn a_chegada_atravessa_o_canal_local() {
    let aviso = ParaInterface::Aviso(chegando());
    let bytes = crate::codec::codificar(&aviso).unwrap();
    let corpo = &bytes[crate::codec::PREFIXO..];
    assert_eq!(
        crate::codec::decodificar::<ParaInterface>(corpo).unwrap(),
        aviso
    );
}
