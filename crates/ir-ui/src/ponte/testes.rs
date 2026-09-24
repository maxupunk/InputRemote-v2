//! A tradução do estado do serviço para o que a tela desenha.

use ir_ipc::status::{Latencia, MotivoDaQueda};
use ir_ipc::vocabulario::{Maquina, Nivel, Nome};

use super::*;

fn recem_instalado() -> Estado {
    Estado::recem_instalado(Maquina([0xAB; 16]), Nome::coagido("bancada"))
}

#[test]
fn o_wifi_do_par_cochilando_vira_aviso_com_o_botao_apontado_para_ele() {
    let mut estado = recem_instalado();
    assert_eq!(estado_ui(&estado).aviso_de_rede, "");

    estado.economia_no_par = Some(ir_ipc::EconomiaDoWifi::Ligada);
    let ui = estado_ui(&estado);
    assert!(
        ui.aviso_de_rede.contains("outro computador"),
        "{}",
        ui.aviso_de_rede
    );
    assert!(ui.aviso_de_rede_no_par);
}

#[test]
fn a_primeira_tela_diz_o_primeiro_passo() {
    let ui = estado_ui(&recem_instalado());
    assert!(ui.resumo.contains("Pareie"), "{}", ui.resumo);
    assert!(!ui.tem_par);
    assert!(!ui.conectado);
}

#[test]
fn sem_o_agente_nada_mais_importa_para_a_cor() {
    // Mesmo com sessão de pé, se o componente que digita não subiu o produto não funciona.
    let mut estado = recem_instalado();
    estado.enlace = LinkState::Controlando;
    estado.agente_pronto = false;
    assert_eq!(saude(&estado), SAUDE_RUIM);
}

#[test]
fn encerrar_por_vontade_do_usuario_nao_pinta_de_vermelho() {
    // Pintar de vermelho o que o usuário acabou de pedir ensina a ignorar o vermelho — e aí a
    // queda que importa passa batida.
    let mut estado = recem_instalado();
    estado.agente_pronto = true;
    estado.ultima_queda = Some(MotivoDaQueda::PedidoPeloUsuario);
    assert_eq!(saude(&estado), SAUDE_ATENCAO);

    estado.ultima_queda = Some(MotivoDaQueda::MeioFalhou);
    assert_eq!(saude(&estado), SAUDE_RUIM);
}

#[test]
fn conectando_tem_saude_de_andamento() {
    let mut estado = recem_instalado();
    estado.agente_pronto = true;
    estado.enlace = LinkState::Conectando;
    assert_eq!(saude(&estado), SAUDE_ANDAMENTO);
}

#[test]
fn as_quatro_bordas_sobrevivem_a_ida_e_volta() {
    for borda in Borda::TODAS {
        assert_eq!(borda_do_indice(indice_da_borda(borda)), borda);
    }
}

#[test]
fn indice_de_borda_invalido_nao_derruba_a_janela() {
    for indice in [-7, 4, i32::MAX, i32::MIN] {
        assert_eq!(borda_do_indice(indice), Borda::Direita);
    }
}

#[test]
fn as_tres_opcoes_de_conexao_sobrevivem_a_ida_e_volta() {
    for escolha in [None, Some(Portador::Bluetooth), Some(Portador::RedeLocal)] {
        assert_eq!(portador_do_indice(indice_do_portador(escolha)), escolha);
    }
}

#[test]
fn a_rede_de_arquivos_nao_e_oferecida_como_meio_de_teclado() {
    // Ela não carrega entrada, então não pode ser escolhível. Se o estado disser que é ela, a
    // tela mostra "Rede" — mas escolher "Rede" nunca produz a rede de arquivos.
    assert_eq!(indice_do_portador(Some(Portador::RedeDeArquivos)), 2);
    assert_eq!(portador_do_indice(2), Some(Portador::RedeLocal));
    assert!(!Portador::RedeDeArquivos.serve_para_entrada());
}

#[test]
fn sem_medida_a_latencia_nao_e_acusada_de_ruim() {
    let estado = recem_instalado();
    assert!(latencia_boa(&estado));
    assert_eq!(estado_ui(&estado).latencia, "");
}

#[test]
fn a_latencia_e_julgada_contra_a_meta_do_portador_em_uso() {
    let mut estado = recem_instalado();
    estado.latencia = Some(Latencia {
        mediana_ms: 15,
        p99_ms: 40,
        amostras: 400,
    });

    estado.portador = Some(Portador::Bluetooth);
    assert!(
        latencia_boa(&estado),
        "15 ms está dentro da meta do Bluetooth"
    );

    estado.portador = Some(Portador::RedeLocal);
    assert!(
        !latencia_boa(&estado),
        "15 ms na rede local está fora da meta"
    );
}

#[test]
fn o_impedimento_vira_texto_vazio_quando_nao_ha_nenhum() {
    // A tela decide mostrar a faixa comparando com "". Se o "nenhum" viesse como a palavra
    // "None", a faixa apareceria sempre.
    let mut estado = recem_instalado();
    estado.agente_pronto = true;
    estado.captura_pronta = true;
    assert_eq!(estado_ui(&estado).impedimento, "");
}

#[test]
fn a_tela_recebe_a_impressao_agrupada_e_o_nivel_por_extenso() {
    let mut estado = recem_instalado();
    estado.nivel_privilegiado = Nivel::TelaDeBloqueio;
    let ui = estado_ui(&estado);

    assert_eq!(ui.nivel, "N2");
    assert!(ui.nivel_suficiente);
    assert!(
        !ui.nivel_explicacao.is_empty(),
        "um selo sem explicação não informa nada"
    );
    assert_eq!(
        ui.esta_impressao.split(' ').count(),
        8,
        "{}",
        ui.esta_impressao
    );
}

#[test]
fn a_dica_da_bandeja_diz_o_estado_e_cabe_no_limite_do_windows() {
    let dica = dica_da_bandeja(
        "Pronto",
        "Conectado a notebook. Leve o ponteiro até a borda.",
    );
    assert!(dica.starts_with("InputRemote — Pronto"));
    let longa = dica_da_bandeja("Desconectado", &"palavra ".repeat(40));
    assert!(longa.chars().count() <= 120, "{}", longa.chars().count());
    assert!(longa.ends_with('…'));
}

#[test]
fn nenhuma_frase_nova_tem_buraco_de_espacos() {
    // A continuação de linha das frases longas já se perdeu em edições por script; o sintoma é
    // um buraco de espaços no meio da frase.
    assert!(
        !AVISO_DO_BLOQUEIO_DO_PAR.contains("  "),
        "buraco em: {AVISO_DO_BLOQUEIO_DO_PAR}"
    );
}
