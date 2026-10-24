//! A cópia que chega: prometida no clipboard enquanto chega, e desfeita se parar.

use super::*;

fn chegando(nome: &str, fase: Fase) -> Transferencia {
    Transferencia {
        sentido: Sentido::Recebendo,
        nome: nome.to_owned(),
        bytes_feitos: 10,
        bytes_total: 100,
        fase,
    }
}

fn aviso_de_chegada(nome: &str) -> ir_ipc::Chegando {
    ir_ipc::Chegando {
        nome: nome.to_owned(),
        montagem: "C:/ProgramData/InputRemote/recebidos/.parcial-7".to_owned(),
        publicada_em: "C:/ProgramData/InputRemote/recebidos".to_owned(),
        itens: vec![
            ir_ipc::ItemChegando {
                caminho: nome.to_owned(),
                tamanho: 0,
                pasta: true,
            },
            ir_ipc::ItemChegando {
                caminho: format!("{nome}/a.png"),
                tamanho: 3,
                pasta: false,
            },
        ],
    }
}

#[test]
fn arquivos_que_comecam_a_chegar_sao_prometidos_no_clipboard() {
    // Colar antes de a cópia chegar: o clipboard já tem os arquivos, lidos à medida que chegam.
    let mut clip = Mentira::default();
    prometer(&aviso_de_chegada("Jogo eletronica 2"), &mut clip);
    let prometido = clip.prometido.expect("prometeu");
    assert_eq!(prometido.itens.len(), 2);
    assert_eq!(
        prometido.itens.get(1).map(|item| item.caminho.as_str()),
        Some("Jogo eletronica 2/a.png")
    );
    assert_eq!(
        prometido.montagem,
        PathBuf::from("C:/ProgramData/InputRemote/recebidos/.parcial-7")
    );
    assert!(
        clip.publicado.is_empty(),
        "o que chegou só é publicado no fim"
    );
}

#[test]
fn a_imagem_que_chega_nao_e_prometida_como_arquivo() {
    let mut clip = Mentira::default();
    let nome = format!("{}20261002.png", imagem::PREFIXO);
    prometer(&aviso_de_chegada(&nome), &mut clip);
    assert_eq!(
        clip.promessas, 0,
        "ela vira imagem no clipboard, e não arquivo"
    );
}

#[test]
fn o_andamento_sozinho_nao_promete_nada() {
    // A promessa precisa da lista de itens, que vem no aviso de chegada.
    let mut clip = Mentira::default();
    reagir(
        &chegando("Jogo eletronica 2", Fase::Andando),
        &mut clip,
        &mut Eco::nova(),
    );
    assert_eq!(clip.promessas, 0);
}

#[test]
fn a_copia_que_para_desfaz_a_promessa() {
    let mut clip = Mentira::default();
    let parada = Fase::Parada(ir_ipc::transferencia::Motivo::CanalCaiu);
    reagir(
        &chegando("Jogo eletronica 2", parada),
        &mut clip,
        &mut Eco::nova(),
    );
    assert_eq!(
        clip.desfeitas, 1,
        "quem estava colando não fica esperando para sempre"
    );
}

#[test]
fn a_copia_que_espera_a_conexao_mantem_a_promessa() {
    // Ela pode recomeçar: desfazer agora faria colar trazer o conteúdo velho de novo.
    let mut clip = Mentira::default();
    reagir(
        &chegando("Jogo eletronica 2", Fase::AguardandoConexao),
        &mut clip,
        &mut Eco::nova(),
    );
    assert_eq!((clip.promessas, clip.desfeitas), (0, 0));
}
