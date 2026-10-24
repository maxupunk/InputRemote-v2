use super::*;

fn amostra(cpu: u16) -> UsageSample {
    UsageSample {
        cpu_permille: Some(cpu),
        ..UsageSample::default()
    }
}

#[test]
fn sem_par_o_quadro_tem_so_este_computador_e_sem_enlace() {
    let mut mesa = Mesa::nova("DESKTOP");
    let agora = Instant::now();
    assert_eq!(
        mesa.quadro(agora).maquinas[0].situacao,
        Situacao::Aguardando
    );
    mesa.anotar_local(amostra(100));
    let quadro = mesa.quadro(agora);
    assert_eq!(quadro.maquinas.len(), 1);
    assert_eq!(quadro.maquinas[0].situacao, Situacao::Amostra(amostra(100)));
    assert!(quadro.enlace.is_none());
}

#[test]
fn o_par_aparece_aguardando_ate_a_primeira_amostra_e_de_novo_se_ela_envelhece() {
    let mut mesa = Mesa::nova("DESKTOP");
    let inicio = Instant::now();
    mesa.par_conectado("notebook", true);
    assert_eq!(
        mesa.quadro(inicio).maquinas[1].situacao,
        Situacao::Aguardando
    );

    mesa.anotar_par(inicio, amostra(80));
    assert_eq!(
        mesa.quadro(inicio + Duration::from_secs(1)).maquinas[1].situacao,
        Situacao::Amostra(amostra(80))
    );
    assert_eq!(
        mesa.quadro(inicio + AMOSTRA_VELHA).maquinas[1].situacao,
        Situacao::Aguardando
    );
}

#[test]
fn par_antigo_aparece_pedindo_atualizacao() {
    let mut mesa = Mesa::nova("DESKTOP");
    mesa.par_conectado("notebook", false);
    mesa.anotar_par(Instant::now(), amostra(1));
    let quadro = mesa.quadro(Instant::now());
    assert_eq!(quadro.maquinas[1].situacao, Situacao::VersaoAntiga);
}

#[test]
fn a_rota_mais_rapida_vence_e_a_mediana_ignora_o_pico() {
    let mut mesa = Mesa::nova("DESKTOP");
    mesa.par_conectado("notebook", true);
    let agora = Instant::now();
    for us in [7_000, 7_500, 90_000] {
        mesa.anotar_volta(Rota::Bluetooth, agora, us);
    }
    for us in [3_000, 3_200, 2_800] {
        mesa.anotar_volta(Rota::Rede, agora, us);
    }
    let enlace = mesa.quadro(agora).enlace.unwrap();
    assert_eq!(enlace.bluetooth_us, Some(7_500));
    assert_eq!(enlace.rede_us, Some(3_000));
    assert_eq!(enlace.vencendo, Some(Rota::Rede));
}

#[test]
fn voltas_velhas_saem_da_janela() {
    let mut mesa = Mesa::nova("DESKTOP");
    mesa.par_conectado("notebook", true);
    let inicio = Instant::now();
    mesa.anotar_volta(Rota::Bluetooth, inicio, 5_000);
    let depois = inicio + JANELA + Duration::from_secs(1);
    let enlace = mesa.quadro(depois).enlace.unwrap();
    assert_eq!(enlace.bluetooth_us, None);
    assert_eq!(enlace.vencendo, None);
}

#[test]
fn a_queda_do_par_esquece_as_voltas() {
    let mut mesa = Mesa::nova("DESKTOP");
    mesa.par_conectado("notebook", true);
    let agora = Instant::now();
    mesa.anotar_volta(Rota::Rede, agora, 3_000);
    mesa.par_desconectado();
    assert!(mesa.quadro(agora).enlace.is_none());
    mesa.par_conectado("notebook", true);
    assert_eq!(mesa.quadro(agora).enlace.unwrap().rede_us, None);
}
