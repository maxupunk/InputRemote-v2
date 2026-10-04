//! O ajudante contra um clipboard de mentira.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use ir_ipc::{ParaInterface, codec};

use super::*;

/// Um clipboard de mentira que lembra o que foi publicado e devolve o que tem.
#[derive(Debug, Default)]
struct Mentira {
    dentro: Option<Conteudo>,
    publicado: Vec<Conteudo>,
    /// Quantas vezes prometeu arquivos, e quantas desfez; e a última chegada prometida.
    promessas: usize,
    desfeitas: usize,
    prometido: Option<ir_clip::Chegada>,
}

impl Clipboard for Mentira {
    fn ler(&mut self) -> ir_clip::Result<Option<Conteudo>> {
        Ok(self.dentro.clone())
    }

    fn publicar(&mut self, conteudo: &Conteudo) -> ir_clip::Result<()> {
        self.dentro = Some(conteudo.clone());
        self.publicado.push(conteudo.clone());
        Ok(())
    }

    fn prometer_arquivos(&mut self, chegada: &ir_clip::Chegada) -> ir_clip::Result<()> {
        self.prometido = Some(chegada.clone());
        self.promessas += 1;
        Ok(())
    }

    fn desfazer_promessa(&mut self) {
        self.desfeitas += 1;
    }
}

/// Os pedidos que saíram, decodificados.
fn pedidos(saida: &[u8]) -> Vec<Pedido> {
    let mut resto = saida;
    let mut todos = Vec::new();
    while resto.len() >= codec::PREFIXO {
        let (prefixo, depois) = resto.split_at(codec::PREFIXO);
        let tamanho = codec::tamanho_anunciado(prefixo).unwrap();
        let (corpo, depois) = depois.split_at(tamanho);
        todos.push(codec::decodificar(corpo).unwrap());
        resto = depois;
    }
    todos
}

/// Nenhuma pasta compartilhada: a cópia de sempre.
fn nenhuma() -> super::pasta::Pastas {
    super::pasta::Pastas::default()
}

fn arquivos(caminho: &str) -> Conteudo {
    Conteudo::Arquivos(vec![PathBuf::from(caminho)])
}

#[test]
fn copiar_de_dentro_da_pasta_compartilhada_nao_manda_arquivos() {
    let pasta = ir_ipc::pastas::IdDePasta([3; 16]);
    let pastas = super::pasta::Pastas::com(pasta, PathBuf::from("/home/maxuel/InputRemote/Temp"));
    let mut clip = Mentira {
        dentro: Some(arquivos("/home/maxuel/InputRemote/Temp/img2.jpg")),
        ..Mentira::default()
    };
    let mut saida = Vec::new();
    oferecer(&mut saida, &mut clip, &mut Eco::nova(), &pastas);
    assert_eq!(
        pedidos(&saida),
        vec![Pedido::Pasta(ir_ipc::pastas::ComandoDePasta::Copiado {
            pasta,
            caminhos: vec!["img2.jpg".to_owned()],
        })]
    );
}

#[test]
fn arquivos_no_clipboard_viram_pedido_de_envio() {
    let mut clip = Mentira {
        dentro: Some(arquivos("/home/maxuel/relatorio")),
        ..Mentira::default()
    };
    let mut saida = Vec::new();
    oferecer(&mut saida, &mut clip, &mut Eco::nova(), &nenhuma());
    assert_eq!(
        pedidos(&saida),
        vec![Pedido::EnviarArquivos {
            caminhos: vec!["/home/maxuel/relatorio".to_owned()]
        }]
    );
}

#[test]
fn a_mesma_copia_nao_sai_duas_vezes() {
    // Cada travessia lê o clipboard. Sem a guarda, cada ida do mouse até a outra tela mandaria
    // a pasta inteira de novo.
    let mut clip = Mentira {
        dentro: Some(arquivos("/home/maxuel/relatorio")),
        ..Mentira::default()
    };
    let mut eco = Eco::nova();
    let mut saida = Vec::new();
    for _ in 0..5 {
        oferecer(&mut saida, &mut clip, &mut eco, &nenhuma());
    }
    assert_eq!(pedidos(&saida).len(), 1);
}

#[test]
fn texto_no_clipboard_vira_oferta_de_texto() {
    let mut clip = Mentira {
        dentro: Some(Conteudo::texto("uma frase\r\ncom quebra")),
        ..Mentira::default()
    };
    let mut saida = Vec::new();
    oferecer(&mut saida, &mut clip, &mut Eco::nova(), &nenhuma());
    // Canônico em LF, qualquer que seja o sistema: é o que o protocolo leva.
    assert_eq!(
        pedidos(&saida),
        vec![Pedido::OferecerTexto(
            TextoDoClipboard::new("uma frase\ncom quebra".to_owned()).unwrap()
        )]
    );
}

#[test]
fn texto_grande_demais_nao_sai_e_nao_fica_marcado() {
    let grande = Conteudo::texto(&"a".repeat(TextoDoClipboard::MAX + 1));
    let mut clip = Mentira {
        dentro: Some(grande.clone()),
        ..Mentira::default()
    };
    let mut eco = Eco::nova();
    let mut saida = Vec::new();
    oferecer(&mut saida, &mut clip, &mut eco, &nenhuma());
    assert!(pedidos(&saida).is_empty());
    assert!(eco.oferecer(&grande), "ficou marcado sem ter ido");
}

#[test]
fn texto_que_chega_vai_para_o_clipboard_e_nao_volta() {
    let mut clip = Mentira::default();
    let mut eco = Eco::nova();
    publicar(&Conteudo::texto("do outro lado"), &mut clip, &mut eco);
    assert_eq!(clip.publicado, vec![Conteudo::texto("do outro lado")]);

    let mut saida = Vec::new();
    oferecer(&mut saida, &mut clip, &mut eco, &nenhuma());
    assert!(pedidos(&saida).is_empty(), "o eco voltou para o par");
}

#[test]
fn o_que_chega_vai_para_o_clipboard_e_nao_volta() {
    // O ciclo inteiro do lado que recebe: publica o que chegou, e a leitura seguinte — a da
    // próxima travessia — não devolve ao par o que veio dele.
    let mut clip = Mentira::default();
    let mut eco = Eco::nova();
    let concluida = Transferencia {
        sentido: Sentido::Recebendo,
        nome: "relatorio".to_owned(),
        bytes_feitos: 10,
        bytes_total: 10,
        fase: Fase::Concluida {
            destino: "/var/lib/inputremote/recebidos/relatorio".to_owned(),
        },
    };
    reagir(&concluida, &mut clip, &mut eco);
    assert_eq!(
        clip.publicado,
        vec![arquivos("/var/lib/inputremote/recebidos/relatorio")]
    );

    let mut saida = Vec::new();
    oferecer(&mut saida, &mut clip, &mut eco, &nenhuma());
    assert!(pedidos(&saida).is_empty(), "o eco voltou para o par");
}

#[test]
fn uma_conclusao_do_lado_que_envia_nao_publica_nada() {
    // Quem envia não sabe onde o arquivo ficou do outro lado; o destino vem vazio, e não há o
    // que pôr no clipboard daqui.
    let mut clip = Mentira::default();
    let enviada = Transferencia {
        sentido: Sentido::Enviando,
        nome: "relatorio".to_owned(),
        bytes_feitos: 10,
        bytes_total: 10,
        fase: Fase::Concluida {
            destino: String::new(),
        },
    };
    reagir(&enviada, &mut clip, &mut Eco::nova());
    assert!(clip.publicado.is_empty());
}

#[test]
fn um_envio_que_falhou_pode_ser_repetido_pela_mesma_copia() {
    let mut clip = Mentira {
        dentro: Some(arquivos("/tmp/x")),
        ..Mentira::default()
    };
    let mut eco = Eco::nova();
    let mut saida = Vec::new();
    oferecer(&mut saida, &mut clip, &mut eco, &nenhuma());
    let falhou = Transferencia {
        sentido: Sentido::Enviando,
        nome: "x".to_owned(),
        bytes_feitos: 0,
        bytes_total: 1,
        fase: Fase::Parada(ir_ipc::transferencia::Motivo::CanalCaiu),
    };
    reagir(&falhou, &mut clip, &mut eco);
    oferecer(&mut saida, &mut clip, &mut eco, &nenhuma());
    assert_eq!(
        pedidos(&saida).len(),
        2,
        "a falha tinha de liberar a repetição"
    );
}

#[test]
fn uma_imagem_atravessa_como_arquivo_e_chega_como_imagem() {
    // Deste lado: a imagem copiada vira um pedido de envio de um PNG reconhecível.
    let png = b"png de mentira do teste do ajudante".to_vec();
    let mut clip = Mentira {
        dentro: Some(Conteudo::Imagem(png.clone())),
        ..Mentira::default()
    };
    let mut saida = Vec::new();
    oferecer(&mut saida, &mut clip, &mut Eco::nova(), &nenhuma());
    let saiu = pedidos(&saida);
    let [Pedido::EnviarArquivos { caminhos }] = saiu.as_slice() else {
        panic!("a imagem tinha de virar um envio");
    };
    let [caminho] = caminhos.as_slice() else {
        panic!("um arquivo só");
    };
    let enviado = PathBuf::from(caminho);
    assert_eq!(std::fs::read(&enviado).unwrap(), png);

    // Do outro lado: o arquivo chega à pasta de recebidos e volta a ser imagem.
    let recebidos = std::env::temp_dir().join(format!("ir-recebidos-{}", std::process::id()));
    std::fs::create_dir_all(&recebidos).unwrap();
    let chegou = recebidos.join(enviado.file_name().unwrap());
    std::fs::copy(&enviado, &chegou).unwrap();
    let mut outro = Mentira::default();
    let mut eco = Eco::nova();
    let concluida = Transferencia {
        sentido: Sentido::Recebendo,
        nome: "imagem".to_owned(),
        bytes_feitos: 1,
        bytes_total: 1,
        fase: Fase::Concluida {
            destino: chegou.to_string_lossy().into_owned(),
        },
    };
    reagir(&concluida, &mut outro, &mut eco);
    assert_eq!(outro.publicado, vec![Conteudo::Imagem(png)]);
    assert!(!chegou.exists(), "a imagem recebida não fica na pasta");

    // E a leitura seguinte não devolve a imagem ao par.
    let mut volta = Vec::new();
    oferecer(&mut volta, &mut outro, &mut eco, &nenhuma());
    assert!(
        pedidos(&volta).is_empty(),
        "o eco da imagem voltou para o par"
    );
    let _ = std::fs::remove_dir_all(recebidos);
}

#[test]
fn um_quadro_que_nao_decodifica_e_versao_diferente_e_um_canal_que_cai_nao() {
    // Um quadro inteiro, de tamanho válido, com corpo que não é `ParaInterface`: é o serviço
    // atualizado falando com um ajudante de antes.
    let mut estranho: &[u8] = &[3, 0, 0, 0, 0xFF, 0xFF, 0xFF];
    let erro = crate::ler_quadro::<ParaInterface>(&mut estranho).unwrap_err();
    assert!(servico::e_incompativel(&erro), "{erro:?}");

    // O canal que acaba no meio do corpo é queda, não versão: reconectar é o certo.
    let mut cortado: &[u8] = &[9, 0, 0, 0, 1];
    let erro = crate::ler_quadro::<ParaInterface>(&mut cortado).unwrap_err();
    assert!(!servico::e_incompativel(&erro), "{erro:?}");
}

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
