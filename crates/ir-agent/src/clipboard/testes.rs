//! O ajudante contra um clipboard de mentira.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

/// Um clipboard de mentira que lembra o que foi publicado e devolve o que tem.
#[derive(Debug, Default)]
struct Mentira {
    dentro: Option<Conteudo>,
    publicado: Vec<Conteudo>,
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

fn arquivos(caminho: &str) -> Conteudo {
    Conteudo::Arquivos(vec![PathBuf::from(caminho)])
}

#[test]
fn arquivos_no_clipboard_viram_pedido_de_envio() {
    let mut clip = Mentira {
        dentro: Some(arquivos("/home/maxuel/relatorio")),
        ..Mentira::default()
    };
    let mut saida = Vec::new();
    oferecer(&mut saida, &mut clip, &mut Eco::nova());
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
        oferecer(&mut saida, &mut clip, &mut eco);
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
    oferecer(&mut saida, &mut clip, &mut Eco::nova());
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
    oferecer(&mut saida, &mut clip, &mut eco);
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
    oferecer(&mut saida, &mut clip, &mut eco);
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
    oferecer(&mut saida, &mut clip, &mut eco);
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
    oferecer(&mut saida, &mut clip, &mut eco);
    let falhou = Transferencia {
        sentido: Sentido::Enviando,
        nome: "x".to_owned(),
        bytes_feitos: 0,
        bytes_total: 1,
        fase: Fase::Parada(ir_ipc::transferencia::Motivo::CanalCaiu),
    };
    reagir(&falhou, &mut clip, &mut eco);
    oferecer(&mut saida, &mut clip, &mut eco);
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
    oferecer(&mut saida, &mut clip, &mut Eco::nova());
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
    oferecer(&mut volta, &mut outro, &mut eco);
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
    assert!(e_incompativel(&erro), "{erro:?}");

    // O canal que acaba no meio do corpo é queda, não versão: reconectar é o certo.
    let mut cortado: &[u8] = &[9, 0, 0, 0, 1];
    let erro = crate::ler_quadro::<ParaInterface>(&mut cortado).unwrap_err();
    assert!(!e_incompativel(&erro), "{erro:?}");
}
