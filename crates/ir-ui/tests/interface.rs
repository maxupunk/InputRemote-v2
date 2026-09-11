//! O que a interface faz, do ponto de vista de quem a usa.
//!
//! Estes testes conduzem o fluxo inteiro — procurar, comparar o código, conectar, encerrar,
//! esquecer — contra o serviço simulado, e verificam o que a tela mostraria em cada ponto. Não
//! abrem janela: o que está sob teste é a tradução de estado em texto, que é onde o usuário ganha
//! ou perde a capacidade de entender o que está acontecendo.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use ir_ipc::status::Papel;
use ir_ipc::vocabulario::Portador;
use ir_ipc::{Aviso, Estado, Pedido, Resposta};
use ir_ui::ponte;
use ir_ui::servico::{Servico, Situacao};
use ir_ui::simulado::ServicoSimulado;

/// Teto de passos para as esperas. Generoso, e finito: um teste que gira para sempre por causa de
/// um defeito é pior que um que falha.
const TETO: u32 = 60;

fn estado(servico: &ServicoSimulado) -> Estado {
    match servico.pedir(Pedido::Estado) {
        Resposta::Estado(estado) => estado,
        outra => panic!("esperava estado, veio {outra:?}"),
    }
}

/// Avança o relógio simulado até um aviso satisfazer o critério, e devolve esse aviso.
fn esperar(servico: &ServicoSimulado, criterio: impl Fn(&Aviso) -> bool) -> Aviso {
    for _ in 0..TETO {
        for aviso in servico.avisos() {
            if criterio(&aviso) {
                return aviso;
            }
        }
    }
    panic!("o aviso esperado não chegou em {TETO} passos");
}

fn achar_candidatos(servico: &ServicoSimulado) -> Vec<ir_ipc::Candidato> {
    assert!(matches!(servico.pedir(Pedido::Procurar), Resposta::Feito));
    let aviso = esperar(servico, |aviso| {
        matches!(aviso, Aviso::CandidatosEncontrados { .. })
    });
    let Aviso::CandidatosEncontrados { candidatos } = aviso else {
        panic!("variante errada");
    };
    assert!(!candidatos.is_empty(), "a descoberta precisa achar alguém");
    candidatos
}

fn pedir_codigo(servico: &ServicoSimulado) -> [u8; 6] {
    let candidatos = achar_candidatos(servico);
    let pedido = Pedido::IniciarPareamento {
        candidato: candidatos[0].endereco.clone(),
    };
    assert!(matches!(servico.pedir(pedido), Resposta::Feito));

    let aviso = esperar(servico, |aviso| {
        matches!(aviso, Aviso::CodigoDePareamento { .. })
    });
    let Aviso::CodigoDePareamento { digitos } = aviso else {
        panic!("variante errada");
    };
    digitos
}

/// Leva a sessão do zero até conectada, do jeito que o usuário levaria.
fn conectar(servico: &ServicoSimulado) {
    let _ = pedir_codigo(servico);
    let confirmar = Pedido::ConfirmarPareamento { conferiu: true };
    assert!(matches!(servico.pedir(confirmar), Resposta::Feito));
    esperar(
        servico,
        |aviso| matches!(aviso, Aviso::EstadoMudou(estado) if estado.enlace.conectado()),
    );
}

#[test]
fn a_primeira_tela_diz_o_que_fazer_primeiro() {
    // Uma máquina recém-instalada não tem nada configurado, e a tela não pode simplesmente dizer
    // "desconectado": isso descreve o problema e esconde a solução.
    let servico = ServicoSimulado::new();
    let tela = ponte::estado_ui(&estado(&servico));

    assert!(tela.resumo.contains("Pareie"), "{}", tela.resumo);
    assert!(!tela.tem_par);
    assert!(!tela.conectado);
    assert_eq!(tela.latencia, "");
}

#[test]
fn o_codigo_de_pareamento_tem_seis_digitos_de_um_algarismo() {
    // A tela mostra um dígito por caixa. Um valor acima de 9 apareceria cortado ou fora da caixa,
    // e o usuário compararia coisa errada.
    let servico = ServicoSimulado::new();
    let digitos = pedir_codigo(&servico);

    assert_eq!(digitos.len(), 6);
    for digito in digitos {
        assert!(digito <= 9, "{digito} não cabe numa caixa de um algarismo");
    }
}

#[test]
fn conectar_leva_a_tela_a_dizer_como_atravessar() {
    let servico = ServicoSimulado::new();
    conectar(&servico);
    let tela = ponte::estado_ui(&estado(&servico));

    assert!(tela.conectado);
    assert!(tela.tem_par);
    assert_eq!(tela.saude, ponte::SAUDE_BOA);
    // Conectado sem dizer o que fazer é um produto que o usuário não sabe usar.
    assert!(tela.resumo.contains("borda"), "{}", tela.resumo);
    assert!(
        !tela.latencia.is_empty(),
        "sem atraso medido não há o que diagnosticar"
    );
    assert!(tela.latencia_boa);
    assert!(
        !tela.motivo_do_portador.is_empty(),
        "o motivo da escolha precisa aparecer"
    );
}

#[test]
fn codigos_diferentes_recusam_o_pareamento_e_nao_convidam_a_repetir() {
    let servico = ServicoSimulado::new();
    let _ = pedir_codigo(&servico);

    let recusa = servico.pedir(Pedido::ConfirmarPareamento { conferiu: false });
    let Resposta::Falha(falha) = recusa else {
        panic!("códigos diferentes precisam falhar, veio {recusa:?}");
    };
    // A instrução não pode ser "tente de novo": códigos diferentes é sinal de alguém no meio, e
    // insistir é exatamente o que não se deve fazer.
    let acao = falha.o_que_fazer();
    assert!(acao.contains("meio"), "{acao}");
    assert!(!acao.contains("de novo"), "{acao}");
    assert!(estado(&servico).par.is_none(), "nada pode ter sido pareado");
}

#[test]
fn encerrar_por_vontade_do_usuario_nao_parece_defeito() {
    let servico = ServicoSimulado::new();
    conectar(&servico);
    assert!(matches!(servico.pedir(Pedido::Encerrar), Resposta::Feito));

    let tela = ponte::estado_ui(&estado(&servico));
    assert!(!tela.conectado);
    assert_ne!(tela.saude, ponte::SAUDE_RUIM, "o usuário pediu isso");
    assert!(tela.tem_par, "encerrar não desfaz o pareamento");
}

#[test]
fn esquecer_o_par_volta_a_tela_ao_comeco() {
    let servico = ServicoSimulado::new();
    conectar(&servico);
    let par = estado(&servico).par.expect("par").maquina;

    assert!(matches!(
        servico.pedir(Pedido::EsquecerPar { maquina: par }),
        Resposta::Feito
    ));

    let tela = ponte::estado_ui(&estado(&servico));
    assert!(!tela.tem_par);
    assert!(tela.resumo.contains("Pareie"), "{}", tela.resumo);
}

#[test]
fn fixar_o_bluetooth_muda_o_motivo_que_a_tela_explica() {
    let servico = ServicoSimulado::new();
    conectar(&servico);

    let fixar = Pedido::FixarPortador(Some(Portador::Bluetooth));
    assert!(matches!(servico.pedir(fixar), Resposta::Feito));

    let tela = ponte::estado_ui(&estado(&servico));
    assert_eq!(tela.portador_fixado, 1);
    assert!(
        tela.motivo_do_portador.contains("Fixado"),
        "{}",
        tela.motivo_do_portador
    );
}

#[test]
fn desligar_a_tela_de_bloqueio_no_cliente_gera_um_impedimento_acionavel() {
    let servico = ServicoSimulado::new();
    conectar(&servico);
    let par = estado(&servico).par.expect("par").maquina;

    assert!(matches!(
        servico.pedir(Pedido::DefinirPapel(Papel::Cliente)),
        Resposta::Feito
    ));
    let desligar = Pedido::PermitirTelaDeBloqueio {
        maquina: par,
        permitir: false,
    };
    assert!(matches!(servico.pedir(desligar), Resposta::Feito));

    let tela = ponte::estado_ui(&estado(&servico));
    assert!(
        !tela.impedimento.is_empty(),
        "o usuário precisa saber que não vai funcionar"
    );
    assert!(
        tela.impedimento.contains("Preferências"),
        "{}",
        tela.impedimento
    );
    assert_eq!(tela.saude, ponte::SAUDE_ATENCAO);
}

#[test]
fn o_diagnostico_traz_o_que_diagnostica_e_nada_do_que_foi_digitado() {
    // O produto vê senhas. Um relatório que o usuário cola num relato público não pode conter
    // tecla, caractere, coordenada nem clipboard (docs/04 §7).
    let servico = ServicoSimulado::new();
    conectar(&servico);

    let Resposta::Diagnostico(relatorio) = servico.pedir(Pedido::Diagnostico) else {
        panic!("o diagnóstico precisa existir");
    };

    for campo in [
        "portador",
        "motivo",
        "atraso",
        "nivel",
        "agente pronto",
        "ultima queda",
    ] {
        assert!(relatorio.contains(campo), "falta `{campo}` no relatório");
    }

    let minusculo = relatorio.to_lowercase();
    for proibido in [
        "tecla",
        "senha",
        "clipboard",
        "caractere",
        "usage",
        "scancode",
    ] {
        assert!(
            !minusculo.contains(proibido),
            "`{proibido}` não pode estar no diagnóstico"
        );
    }
}

#[test]
fn o_simulado_se_declara_simulado() {
    // A janela mostra um aviso baseado nisto. Se o simulado mentisse, o usuário acreditaria que o
    // produto está funcionando.
    assert_eq!(ServicoSimulado::new().situacao(), Situacao::Simulado);
}
