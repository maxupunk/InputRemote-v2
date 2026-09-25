//! Os testes do estado publicado: as frases, a rota e o que a tela precisa dizer primeiro.

use super::*;

fn pronto() -> Estado {
    let mut estado = Estado::recem_instalado(Maquina([3; 16]), Nome::coagido("notebook"));
    estado.agente_pronto = true;
    estado.captura_pronta = true;
    estado.nivel_privilegiado = Nivel::TelaDeBloqueio;
    estado.bloqueio_permitido = true;
    estado
}

#[test]
fn a_rota_dupla_aparece_com_os_dois_nomes() {
    let mut estado = pronto();
    assert_eq!(estado.nome_da_rota(), "", "sem sessão não há rota");
    estado.portador = Some(Portador::Bluetooth);
    assert_eq!(estado.nome_da_rota(), "Bluetooth");
    estado.rota_dupla = true;
    assert_eq!(estado.nome_da_rota(), "Bluetooth + Rede local");
    assert!(!MotivoDoPortador::Redundancia.frase().is_empty());
}

#[test]
fn sem_receber_o_aviso_diz_que_so_este_controla() {
    // Mostrar "ligue a opção de tela de bloqueio" quando o componente que digita nem subiu manda o
    // usuário resolver o problema errado.
    let mut estado = pronto();
    estado.agente_pronto = false;
    estado.nivel_privilegiado = Nivel::Nenhum;
    estado.bloqueio_permitido = false;
    let frase = estado.impedimento().expect("há impedimento");
    assert!(frase.contains("receber o teclado e o mouse"), "{frase}");
    assert!(frase.contains("só este controla"), "{frase}");
}

#[test]
fn sem_captura_ouve_o_problema_certo_e_nao_manda_levar_o_ponteiro() {
    // O relato do log 47: o Linux dizia "o componente que digita", e o ponteiro parava na borda
    // sem que nada explicasse que era a leitura do teclado daqui que faltava.
    let mut estado = pronto();
    estado.captura_pronta = false;
    estado.agente_pronto = false;
    estado.enlace = LinkState::Pronto;
    estado.par = Some(ParConhecido {
        maquina: Maquina([1; 16]),
        nome: Nome::coagido("SAMSUNG"),
        recursos: Recursos::default(),
        conectado: true,
    });
    let frase = estado.impedimento().expect("há impedimento");
    assert!(frase.contains("ler o próprio teclado e mouse"), "{frase}");
    let resumo = estado.resumo();
    assert!(!resumo.contains("Leve o ponteiro"), "{resumo}");
    assert!(resumo.contains("não atravessam"), "{resumo}");
}

#[test]
fn a_tela_de_bloqueio_desligada_nao_pinta_o_cliente_de_laranja() {
    // Opcional e desligada por padrão: tratá-la como impedimento deixava o controlado sempre em
    // laranja, com tudo funcionando.
    let mut sem_permissao = pronto();
    sem_permissao.bloqueio_permitido = false;
    assert_eq!(sem_permissao.impedimento(), None);
}

#[test]
fn nao_conseguir_e_nao_ter_deixado_sao_explicados_de_jeitos_diferentes() {
    let mut sem_capacidade = pronto();
    sem_capacidade.nivel_privilegiado = Nivel::SoDesbloqueado;

    let mut sem_permissao = pronto();
    sem_permissao.bloqueio_permitido = false;

    assert_ne!(
        sem_capacidade.sobre_a_tela_de_bloqueio(),
        sem_permissao.sobre_a_tela_de_bloqueio(),
        "as duas causas pedem ações diferentes do usuário"
    );
    assert!(sem_permissao.sobre_a_tela_de_bloqueio().contains("Ligue"));
    for estado in [&sem_capacidade, &sem_permissao] {
        assert!(!estado.sobre_a_tela_de_bloqueio().contains("  "));
    }
    assert!(pronto().sobre_a_tela_de_bloqueio().starts_with("Ligada"));
}

#[test]
fn a_pausa_aparece_no_resumo_de_cada_lado() {
    let mut estado = pronto();
    estado.par = Some(ParConhecido {
        maquina: Maquina([1; 16]),
        nome: Nome::coagido("notebook"),
        recursos: Recursos::default(),
        conectado: false,
    });
    estado.pausa = Some(Pausa::Aqui);
    assert!(estado.resumo().contains("retomar"), "{}", estado.resumo());
    estado.pausa = Some(Pausa::NoPar);
    assert_eq!(estado.resumo(), "notebook pausou o compartilhamento.");
}

#[test]
fn quem_e_usado_de_longe_aprende_a_voltar() {
    let mut estado = pronto();
    estado.enlace = LinkState::Controlado;
    estado.par = Some(ParConhecido {
        maquina: Maquina([1; 16]),
        nome: Nome::coagido("desktop"),
        recursos: Recursos::default(),
        conectado: true,
    });
    assert_eq!(
        estado.resumo(),
        "desktop está usando este computador. Mexa no mouse daqui para voltar a usá-lo."
    );
    assert_eq!(estado.frase_do_enlace(), "Controlado pelo outro computador");
    estado.enlace = LinkState::Controlando;
    assert_eq!(estado.resumo(), "Controlando desktop.");
    assert_eq!(estado.frase_do_enlace(), "Controlando o outro computador");
}

#[test]
fn quem_le_e_recebe_nao_tem_impedimento() {
    assert_eq!(pronto().impedimento(), None);
}

#[test]
fn a_politica_decide_o_que_falta_e_o_que_nao() {
    // Quem só controla não precisa receber, e quem só é controlado não precisa ler.
    let mut so_este = pronto();
    so_este.politica = Politica::SoEste;
    so_este.agente_pronto = false;
    assert_eq!(so_este.impedimento(), None);

    let mut so_o_outro = pronto();
    so_o_outro.politica = Politica::SoOOutro;
    so_o_outro.captura_pronta = false;
    assert_eq!(so_o_outro.impedimento(), None);
}

#[test]
fn pronto_diz_o_que_se_pode_fazer_daqui() {
    let mut estado = pronto();
    estado.enlace = LinkState::Pronto;
    estado.par = Some(ParConhecido {
        maquina: Maquina([1; 16]),
        nome: Nome::coagido("desktop"),
        recursos: Recursos::default(),
        conectado: true,
    });
    assert!(estado.resumo().contains("Leve o ponteiro"));
    estado.politica = Politica::SoOOutro;
    assert!(
        estado.resumo().contains("de lá controlam este"),
        "{}",
        estado.resumo()
    );
}

#[test]
fn a_tela_de_bloqueio_nao_e_impedimento() {
    // Ela é opcional: o produto funciona sem ela, e o laranja passaria a não querer dizer nada.
    let mut estado = pronto();
    estado.nivel_privilegiado = Nivel::Nenhum;
    estado.bloqueio_permitido = false;
    assert_eq!(estado.impedimento(), None);
}

#[test]
fn o_portador_em_uso_e_o_fixado_sao_campos_distintos() {
    // Um usuário que fixou Bluetooth e está na rede precisa ver as duas coisas: senão a
    // interface mostra "rede" e a preferência dele parece ter sido ignorada sem explicação.
    let mut estado = pronto();
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
    let mut estado = pronto();
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
    let mut estado = pronto();
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

#[test]
fn a_tela_mostra_um_aviso_so_o_que_mais_impede() {
    // Antes a tela empilhava até três faixas; o componente dela promete no máximo uma.
    let mut estado = pronto();
    assert_eq!(estado.aviso_principal(), None);

    estado.economia_no_par = Some(EconomiaDoWifi::Ligada);
    let rede = estado.aviso_principal().expect("há aviso");
    assert_eq!(rede.resolver_no_par(), Some(true), "o de rede tem o botão");

    estado.par_recusa_tela_de_bloqueio = true;
    let bloqueio = estado.aviso_principal().expect("há aviso");
    assert_eq!(bloqueio, AvisoPrincipal::BloqueioDoPar);
    assert_eq!(bloqueio.resolver_no_par(), None);

    estado.captura_pronta = false;
    let impedimento = estado.aviso_principal().expect("há aviso");
    assert_eq!(Some(impedimento.frase()), estado.impedimento());
}

#[test]
fn as_frases_de_aviso_nao_tem_buraco() {
    assert!(
        !AVISO_DO_BLOQUEIO_DO_PAR.contains("  "),
        "{AVISO_DO_BLOQUEIO_DO_PAR}"
    );
    for borda in Borda::TODAS {
        let frase = frase_da_borda_ajustada(borda);
        assert!(
            frase.starts_with("O outro computador mudou de lado"),
            "{frase}"
        );
        assert!(frase.contains(borda_por_extenso(borda)), "{frase}");
    }
}

fn borda_por_extenso(borda: Borda) -> &'static str {
    match borda {
        Borda::Esquerda => "esquerda",
        Borda::Direita => "direita",
        Borda::Acima => "acima",
        Borda::Abaixo => "abaixo",
    }
}

#[test]
fn so_a_falha_de_verdade_alarma() {
    // O defeito que isto trava: um par suspenso era "normal" na cor da janela e "Conexão perdida"
    // na notificação, porque as duas decidiam por listas diferentes.
    use MotivoDaQueda as Q;
    for motivo in [Q::PedidoPeloUsuario, Q::ParPausou] {
        assert_eq!(motivo.gravidade(), Gravidade::Pedida, "{motivo:?}");
    }
    for motivo in [
        Q::ServicoDoParParando,
        Q::ParSuspenso,
        Q::TrocandoDeMeio,
        Q::EstaMaquinaSuspensa,
        Q::ParRecomecou,
    ] {
        assert_eq!(motivo.gravidade(), Gravidade::Esperada, "{motivo:?}");
    }
    for motivo in [Q::ParNaoRespondeu, Q::MeioFalhou, Q::ErroDeProtocolo] {
        assert_eq!(motivo.gravidade(), Gravidade::Falha, "{motivo:?}");
    }
}

#[test]
fn a_latencia_se_escreve_igual_na_tela_e_com_as_amostras_no_diagnostico() {
    let medida = Latencia {
        mediana_ms: 12,
        p99_ms: 34,
        amostras: 500,
    };
    assert_eq!(medida.frase(), "12 ms · 34 ms no pior caso");
    assert_eq!(
        medida.frase_do_diagnostico(),
        "12 ms de mediana, 34 ms no pior caso (500 amostras em 10 s)"
    );
}
