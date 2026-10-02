#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

fn caminhos(nomes: &[&str]) -> Vec<PathBuf> {
    nomes.iter().map(PathBuf::from).collect()
}

fn pedir(fila: &Fila, nomes: &[&str]) -> Recebido {
    fila.pedir(caminhos(nomes), Leitor::Proprio).0
}

#[test]
fn a_mesma_pasta_pedida_de_novo_nao_vira_outra_copia() {
    // O defeito relatado: Ctrl+C três vezes na mesma pasta copiava três vezes.
    let fila = Fila::default();
    assert_eq!(pedir(&fila, &["/casa/teste"]), Recebido::Aceito);
    assert_eq!(pedir(&fila, &["/casa/teste"]), Recebido::Repetido);
    let trabalho = fila.tomar().expect("há o que copiar");
    assert_eq!(trabalho.caminhos, caminhos(&["/casa/teste"]));
    assert_eq!(
        pedir(&fila, &["/casa/teste"]),
        Recebido::Repetido,
        "agora está indo; pedir de novo não duplica"
    );
    assert!(fila.tomar().is_none(), "não há segunda cópia esperando");
    assert!(!fila.cancelando(), "nada a cancelar");
}

#[test]
fn copiar_outra_coisa_cancela_a_que_esta_indo() {
    let fila = Fila::default();
    assert_eq!(pedir(&fila, &["/casa/teste"]), Recebido::Aceito);
    fila.tomar().expect("começou a primeira");
    assert_eq!(pedir(&fila, &["/casa/ffmpeg"]), Recebido::Substituiu);
    assert!(fila.cancelando(), "a que está indo precisa parar");

    fila.terminou();
    assert!(!fila.cancelando());
    let proxima = fila.tomar().expect("a nova assume");
    assert_eq!(proxima.caminhos, caminhos(&["/casa/ffmpeg"]));
}

#[test]
fn cancelar_para_o_que_esta_indo_e_esquece_o_que_espera() {
    let fila = Fila::default();
    assert!(!fila.cancelar_tudo().0, "sem cópia, nada a cancelar");
    pedir(&fila, &["/casa/a"]);
    fila.tomar().expect("começou");
    pedir(&fila, &["/casa/b"]);
    let (havia, a_vista) = fila.cancelar_tudo();
    assert!(havia);
    assert!(
        a_vista.is_none(),
        "a que esperava atrás da outra nunca apareceu na tela"
    );
    assert!(fila.cancelando(), "a que está indo para");
    fila.terminou();
    assert!(fila.tomar().is_none(), "a que esperava não começa depois");
}

#[test]
fn entre_dois_pedidos_novos_so_o_ultimo_espera() {
    let fila = Fila::default();
    pedir(&fila, &["/casa/a"]);
    fila.tomar().expect("a primeira começou");
    pedir(&fila, &["/casa/b"]);
    pedir(&fila, &["/casa/c"]);
    fila.terminou();
    let proxima = fila.tomar().expect("há o que copiar");
    assert_eq!(proxima.caminhos, caminhos(&["/casa/c"]));
    assert!(fila.tomar().is_none(), "o do meio não ficou guardado");
}

#[test]
fn depois_de_terminar_a_mesma_pasta_pode_ir_de_novo() {
    // Não é repetição acidental: a cópia acabou, e o usuário copiou de novo de propósito.
    let fila = Fila::default();
    pedir(&fila, &["/casa/teste"]);
    fila.tomar();
    fila.terminou();
    assert_eq!(pedir(&fila, &["/casa/teste"]), Recebido::Aceito);
}

#[test]
fn a_ordem_dos_caminhos_faz_parte_do_pedido() {
    let fila = Fila::default();
    pedir(&fila, &["/casa/a", "/casa/b"]);
    fila.tomar();
    assert_eq!(
        pedir(&fila, &["/casa/b", "/casa/a"]),
        Recebido::Substituiu,
        "seleção diferente, ainda que dos mesmos itens"
    );
}

#[test]
fn descartar_tira_o_que_espera_sem_comecar() {
    let fila = Fila::default();
    pedir(&fila, &["/casa/teste"]);
    let descartado = fila.descartar().expect("havia um esperando");
    assert_eq!(descartado.caminhos, caminhos(&["/casa/teste"]));
    assert!(fila.tomar().is_none());
}

/// Começa uma cópia e devolve-a como o enlace a deixaria ao cair no meio.
fn interrompida(fila: &Fila, nomes: &[&str]) -> Trabalho {
    pedir(fila, nomes);
    let mut trabalho = fila.tomar().expect("começou");
    trabalho.quedas += 1;
    trabalho.progresso = (10, 30);
    trabalho
}

#[test]
fn a_copia_que_o_enlace_interrompeu_volta_para_a_fila() {
    // O defeito de experiência: uma queda de dois segundos acabava com a cópia, e a pessoa tinha
    // de copiar de novo.
    let fila = Fila::default();
    let trabalho = interrompida(&fila, &["/casa/jogo"]);
    assert_eq!(fila.devolver(trabalho), Devolucao::VaiDeNovo);
    let de_novo = fila.tomar().expect("vai de novo quando o enlace voltar");
    assert_eq!(de_novo.caminhos, caminhos(&["/casa/jogo"]));
    assert_eq!(
        (de_novo.quedas, de_novo.progresso),
        (1, (10, 30)),
        "lembra quantas vezes já caiu"
    );
}

#[test]
fn a_copia_cancelada_enquanto_ia_nao_volta_quando_o_enlace_cai() {
    let fila = Fila::default();
    let trabalho = interrompida(&fila, &["/casa/jogo"]);
    let _ = fila.cancelar_tudo();
    assert_eq!(fila.devolver(trabalho), Devolucao::Cancelada);
    assert!(fila.tomar().is_none());
}

#[test]
fn a_copia_substituida_enquanto_ia_da_lugar_a_nova() {
    let fila = Fila::default();
    let trabalho = interrompida(&fila, &["/casa/jogo"]);
    pedir(&fila, &["/casa/outra"]);
    assert_eq!(fila.devolver(trabalho), Devolucao::Cancelada);
    let proxima = fila.tomar().expect("a nova continua na vez");
    assert_eq!(proxima.caminhos, caminhos(&["/casa/outra"]));
}

#[test]
fn a_copia_largada_no_meio_e_recuperada_e_nao_fica_presa_em_curso() {
    let fila = Fila::default();
    pedir(&fila, &["/casa/jogo"]);
    fila.tomar().expect("começou");
    let largada = fila.abandonada().expect("estava em curso");
    assert_eq!(largada.caminhos, caminhos(&["/casa/jogo"]));
    assert_eq!(
        pedir(&fila, &["/casa/jogo"]),
        Recebido::Aceito,
        "pedir de novo não é tomado por repetição de uma cópia que ninguém conduz"
    );
    assert!(fila.abandonada().is_none());
}

#[test]
fn a_espera_aparece_uma_vez_so() {
    let fila = Fila::default();
    assert!(fila.mostrar_espera().is_none(), "sem cópia, nada a mostrar");
    pedir(&fila, &["/casa/a"]);
    let espera = fila.mostrar_espera().expect("a cópia espera a conexão");
    assert_eq!(espera.caminhos, caminhos(&["/casa/a"]));
    assert!(
        fila.mostrar_espera().is_none(),
        "o aviso e o prazo dela já correm"
    );
}

#[test]
fn desistir_tira_a_copia_que_esperava_e_so_ela() {
    let fila = Fila::default();
    pedir(&fila, &["/casa/a"]);
    let espera = fila.mostrar_espera().expect("espera");
    assert!(fila.desistir(&espera).is_some());
    assert!(fila.tomar().is_none());
}

#[test]
fn a_desistencia_atrasada_nao_acha_a_copia_que_ja_recomecou() {
    // A cópia esperou, o enlace voltou, ela recomeçou e caiu de novo: o prazo da primeira espera
    // não pode derrubar a segunda antes da hora dela.
    let fila = Fila::default();
    pedir(&fila, &["/casa/a"]);
    let primeira = fila.mostrar_espera().expect("espera");
    let trabalho = fila.tomar().expect("recomeçou");
    assert_eq!(fila.devolver(trabalho), Devolucao::VaiDeNovo);
    assert!(fila.desistir(&primeira).is_none());
    assert!(
        fila.mostrar_espera().is_some(),
        "a segunda espera é outra, com prazo próprio"
    );
}

#[test]
fn a_desistencia_nao_acha_a_copia_que_tomou_o_lugar() {
    let fila = Fila::default();
    pedir(&fila, &["/casa/a"]);
    let espera = fila.mostrar_espera().expect("espera");
    let (_, saiu) = fila.pedir(caminhos(&["/casa/b"]), Leitor::Proprio);
    assert!(
        saiu.is_some_and(|t| t.caminhos == caminhos(&["/casa/a"])),
        "a que estava na tela sai, e quem pediu diz isso"
    );
    assert!(fila.desistir(&espera).is_none());
    assert_eq!(
        fila.tomar().map(|t| t.caminhos),
        Some(caminhos(&["/casa/b"]))
    );
}

#[test]
fn cancelar_a_copia_que_esperava_na_tela_a_devolve_para_ser_contada() {
    let fila = Fila::default();
    pedir(&fila, &["/casa/a"]);
    fila.mostrar_espera().expect("espera");
    let (havia, a_vista) = fila.cancelar_tudo();
    assert!(havia);
    assert!(
        a_vista.is_some(),
        "senão o cartão fica esperando para sempre"
    );
}
