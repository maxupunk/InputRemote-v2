//! Os testes do estado publicado: as frases, a rota e o que a tela precisa dizer primeiro.

use super::*;

fn cliente_pronto() -> Estado {
    let mut estado = Estado::recem_instalado(Maquina([3; 16]), Nome::coagido("cliente"));
    estado.papel = Papel::Cliente;
    estado.agente_pronto = true;
    estado.nivel_privilegiado = Nivel::TelaDeBloqueio;
    estado.bloqueio_permitido = true;
    estado
}

#[test]
fn a_rota_dupla_aparece_com_os_dois_nomes() {
    let mut estado = cliente_pronto();
    assert_eq!(estado.nome_da_rota(), "", "sem sessão não há rota");
    estado.portador = Some(Portador::Bluetooth);
    assert_eq!(estado.nome_da_rota(), "Bluetooth");
    estado.rota_dupla = true;
    assert_eq!(estado.nome_da_rota(), "Bluetooth + Rede local");
    assert!(!MotivoDoPortador::Redundancia.frase().is_empty());
}

#[test]
fn o_agente_ausente_e_o_impedimento_mais_grave() {
    // Sem o agente nada funciona, então ele precisa vencer qualquer outro aviso — mostrar
    // "ligue a opção de tela de bloqueio" quando o componente que digita nem subiu manda o
    // usuário resolver o problema errado.
    let mut estado = cliente_pronto();
    estado.agente_pronto = false;
    estado.nivel_privilegiado = Nivel::Nenhum;
    estado.bloqueio_permitido = false;
    let frase = estado.impedimento().expect("há impedimento");
    assert!(frase.contains("digita nesta máquina"), "{frase}");
}

#[test]
fn nao_conseguir_e_nao_ter_deixado_sao_impedimentos_distintos() {
    let mut sem_capacidade = cliente_pronto();
    sem_capacidade.nivel_privilegiado = Nivel::SoDesbloqueado;

    let mut sem_permissao = cliente_pronto();
    sem_permissao.bloqueio_permitido = false;

    assert_ne!(
        sem_capacidade.impedimento(),
        sem_permissao.impedimento(),
        "as duas causas pedem ações diferentes do usuário"
    );
    assert!(
        sem_permissao
            .impedimento()
            .expect("há impedimento")
            .contains("Preferências")
    );
}

#[test]
fn um_cliente_capaz_e_permitido_nao_tem_impedimento() {
    assert_eq!(cliente_pronto().impedimento(), None);
}

#[test]
fn o_servidor_nao_e_cobrado_pela_tela_de_bloqueio_do_par() {
    // Quem tem o teclado não precisa aceitar digitação na própria tela de bloqueio para o
    // produto cumprir o requisito: é o lado controlado que precisa.
    let mut estado = cliente_pronto();
    estado.papel = Papel::Servidor;
    estado.nivel_privilegiado = Nivel::Nenhum;
    estado.bloqueio_permitido = false;
    assert_eq!(estado.impedimento(), None);
}

#[test]
fn o_portador_em_uso_e_o_fixado_sao_campos_distintos() {
    // Um usuário que fixou Bluetooth e está na rede precisa ver as duas coisas: senão a
    // interface mostra "rede" e a preferência dele parece ter sido ignorada sem explicação.
    let mut estado = cliente_pronto();
    estado.portador_fixado = Some(Portador::Bluetooth);
    estado.portador = Some(Portador::RedeLocal);
    assert_ne!(estado.portador, estado.portador_fixado);
}

#[test]
fn a_meta_de_latencia_e_mais_folgada_no_bluetooth() {
    let medida = Latencia {
        mediana_ms: 15,
        p99_ms: 40,
        amostras: 500,
    };
    assert!(medida.dentro_da_meta(Portador::Bluetooth));
    assert!(
        !medida.dentro_da_meta(Portador::RedeLocal),
        "na rede 15 ms de mediana já é ruim"
    );
}

#[test]
fn o_aviso_de_rede_prefere_o_par_e_some_sem_economia() {
    let mut estado = cliente_pronto();
    assert_eq!(estado.aviso_de_rede(), None);

    estado.economia_aqui = Some(EconomiaDoWifi::SoNaBateria);
    let aqui = estado.aviso_de_rede().expect("há aviso");
    assert!(!aqui.no_par);
    assert!(aqui.frase.contains("na bateria"), "{}", aqui.frase);

    estado.economia_no_par = Some(EconomiaDoWifi::Ligada);
    let par = estado.aviso_de_rede().expect("há aviso");
    assert!(
        par.no_par,
        "quem trava o mouse de quem olha esta tela é a placa do outro"
    );
    assert!(par.frase.contains("outro computador"), "{}", par.frase);
}

#[test]
fn nenhuma_frase_do_aviso_de_rede_tem_espaco_sobrando() {
    // A continuação de linha das frases (`\` no fim) já se perdeu uma vez numa edição, e a janela
    // mostrou "cochila                entre pacotes".
    let mut estado = cliente_pronto();
    for (par, aqui) in [
        (Some(EconomiaDoWifi::Ligada), None),
        (None, Some(EconomiaDoWifi::Ligada)),
        (Some(EconomiaDoWifi::SoNaBateria), None),
        (None, Some(EconomiaDoWifi::SoNaBateria)),
    ] {
        estado.economia_no_par = par;
        estado.economia_aqui = aqui;
        let frase = estado.aviso_de_rede().expect("há aviso").frase;
        assert!(!frase.contains("  "), "{frase}");
    }
}
