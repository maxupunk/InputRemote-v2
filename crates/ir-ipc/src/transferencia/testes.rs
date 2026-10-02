#![allow(clippy::float_cmp)]

use super::*;

fn copia(fase: Fase, sentido: Sentido, feitos: u64, total: u64) -> Transferencia {
    Transferencia {
        sentido,
        nome: "pasta-B".to_owned(),
        bytes_feitos: feitos,
        bytes_total: total,
        fase,
    }
}

#[test]
fn o_andamento_diz_o_que_e_quanto() {
    let copia = copia(Fase::Andando, Sentido::Enviando, 512, 1024);
    assert_eq!(copia.titulo(), "Copiando para o outro computador");
    assert_eq!(copia.detalhe(), "pasta-B · 512 B de 1,0 KB · 50%");
    assert!(!copia.terminou() && !copia.falhou());
}

#[test]
fn quem_recebe_sabe_onde_o_que_chegou_ficou() {
    let destino = if cfg!(windows) {
        r"C:\ProgramData\InputRemote\recebidos\pasta-B"
    } else {
        "/var/lib/inputremote/recebidos/pasta-B"
    };
    let copia = copia(
        Fase::Concluida {
            destino: destino.to_owned(),
        },
        Sentido::Recebendo,
        24,
        24,
    );
    assert_eq!(copia.titulo(), "Chegou: é só colar");
    assert!(
        copia.detalhe().starts_with("pasta-B · em "),
        "{}",
        copia.detalhe()
    );
    assert!(!copia.detalhe().ends_with("pasta-B"), "a pasta, não o item");
    assert!(copia.terminou() && !copia.falhou());
}

/// O defeito que motivou isto: a cópia parava, nada aparecia na tela, e colar do outro lado
/// trazia a cópia **anterior** — que continuava no clipboard de lá.
#[test]
fn uma_copia_que_nao_atravessa_diz_por_que() {
    let copia = copia(Fase::Parada(Motivo::CanalCaiu), Sentido::Enviando, 0, 1024);
    assert_eq!(copia.titulo(), "A cópia não atravessou");
    assert!(
        copia.detalhe().contains("conexão de arquivos caiu"),
        "{}",
        copia.detalhe()
    );
    assert!(copia.terminou() && copia.falhou());
}

#[test]
fn a_copia_que_espera_a_conexao_diz_que_segue_sozinha() {
    let copia = copia(Fase::AguardandoConexao, Sentido::Enviando, 512, 1024);
    assert_eq!(copia.titulo(), "Esperando a conexão voltar");
    assert!(
        copia.detalhe().contains("segue sozinha"),
        "{}",
        copia.detalhe()
    );
    assert!(
        copia.em_curso() && !copia.terminou() && !copia.falhou(),
        "esperar não é falhar: ainda pode chegar, e ainda pode ser cancelada"
    );
}

#[test]
fn o_tamanho_e_legivel_em_cada_faixa() {
    assert_eq!(tamanho_legivel(0), "0 B");
    assert_eq!(tamanho_legivel(512), "512 B");
    assert_eq!(tamanho_legivel(1024), "1,0 KB");
    assert_eq!(tamanho_legivel(1_572_864), "1,5 MB");
    assert_eq!(tamanho_legivel(1_288_490_189), "1,2 GB");
    assert!(tamanho_legivel(u64::MAX).ends_with("GB"));
}

#[test]
fn uma_arvore_so_de_pastas_esta_pronta_e_nao_em_zero_por_cento() {
    let copia = copia(Fase::Andando, Sentido::Enviando, 0, 0);
    assert!(copia.detalhe().ends_with("100%"), "{}", copia.detalhe());
}

fn andando(feitos: u64, total: u64) -> Transferencia {
    Transferencia {
        sentido: Sentido::Recebendo,
        nome: "relatório".to_owned(),
        bytes_feitos: feitos,
        bytes_total: total,
        fase: Fase::Andando,
    }
}

#[test]
fn o_progresso_vai_de_zero_a_um() {
    assert_eq!(andando(0, 100).progresso(), 0.0);
    assert_eq!(andando(50, 100).progresso(), 0.5);
    assert_eq!(andando(100, 100).progresso(), 1.0);
}

#[test]
fn uma_arvore_sem_bytes_esta_pronta_e_nao_em_zero_por_cento() {
    // Copiar uma estrutura de pastas vazia é legítimo, e a barra não pode dividir por zero
    // nem ficar parada no começo para sempre.
    assert_eq!(andando(0, 0).progresso(), 1.0);
}

#[test]
fn a_copia_que_espera_sem_ter_comecado_nao_aparece_pronta() {
    let mut copia = andando(0, 0);
    copia.fase = Fase::AguardandoConexao;
    assert_eq!(copia.progresso(), 0.0);
}

#[test]
fn um_progresso_maior_que_o_total_nao_passa_de_um() {
    // Defesa contra contagem errada: a barra pode estar errada, mas não pode estourar a tela.
    assert_eq!(andando(500, 100).progresso(), 1.0);
}

#[test]
fn todo_motivo_tem_frase_em_portugues_e_nao_vazia() {
    let motivos = [
        Motivo::SemPermissao,
        Motivo::AcimaDaCota,
        Motivo::SemEspaco,
        Motivo::CaminhoInseguro,
        Motivo::ItensDemais,
        Motivo::ResumoDivergente,
        Motivo::Cancelada,
        Motivo::CanalCaiu,
        Motivo::Outro("o disco falhou".to_owned()),
    ];
    for motivo in motivos {
        let frase = motivo.descricao();
        assert!(!frase.is_empty(), "{motivo:?}");
        assert!(
            !frase.contains("Reason") && !frase.contains('_'),
            "`{frase}` parece nome de variante, não frase: a tela não traduz, ela mostra"
        );
    }
}

#[test]
fn so_o_que_ainda_acontece_conta_como_em_curso() {
    let mut t = andando(1, 2);
    assert!(t.em_curso());
    t.fase = Fase::Anunciada;
    assert!(t.em_curso());
    t.fase = Fase::AguardandoConexao;
    assert!(t.em_curso());
    t.fase = Fase::Concluida {
        destino: "C:/x".to_owned(),
    };
    assert!(!t.em_curso());
    t.fase = Fase::Parada(Motivo::Cancelada);
    assert!(!t.em_curso());
}

#[test]
fn o_sentido_tem_rotulo() {
    assert_eq!(Sentido::Enviando.rotulo(), "enviando");
    assert_eq!(Sentido::Recebendo.rotulo(), "recebendo");
}

/// O número de cada fase no fio do canal local. O `postcard` numera pela posição, e um processo
/// de antes de uma atualização — o ajudante de clipboard, que sobrevive a ela — ainda lê os
/// números antigos. Variante nova vai no fim; mudar um número aqui quebra quem já está rodando
/// (log 55: copiar do Linux para o Windows parou assim).
#[test]
fn o_numero_de_cada_fase_no_fio_nao_muda() {
    let numero = |fase: Fase| postcard::to_allocvec(&fase).unwrap()[0];
    assert_eq!(numero(Fase::Anunciada), 0);
    assert_eq!(numero(Fase::Andando), 1);
    assert_eq!(
        numero(Fase::Concluida {
            destino: String::new()
        }),
        2
    );
    assert_eq!(numero(Fase::Parada(Motivo::Cancelada)), 3);
    assert_eq!(numero(Fase::AguardandoConexao), 4);
}
