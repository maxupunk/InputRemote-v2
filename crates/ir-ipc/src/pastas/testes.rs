use super::*;
use crate::codec;

const PASTA: IdDePasta = IdDePasta([9; 16]);

fn numero<T: Serialize>(valor: &T) -> u8 {
    postcard::to_allocvec(valor).unwrap()[0]
}

/// O defeito que esta regra evita: um ajudante que sobreviveu a uma atualização lia uma variante
/// renumerada como mensagem malformada ([log 56](../../../../docs/logs/56-o-ajudante-que-sobreviveu.md)).
#[test]
fn o_numero_de_cada_variante_no_fio_nao_muda() {
    assert_eq!(
        numero(&DoAjudanteDePastas::Apresentar { pastas: vec![] }),
        0
    );
    assert_eq!(
        numero(&DoAjudanteDePastas::ParaOPar(FolderMessage::HelperAbsent)),
        1
    );
    assert_eq!(numero(&DoAjudanteDePastas::Resumo(vec![])), 2);
    assert_eq!(numero(&DoAjudanteDePastas::Recado(String::new())), 3);
    assert_eq!(numero(&DoAjudanteDePastas::PorNoClipboard(vec![])), 4);

    assert_eq!(
        numero(&ParaOAjudanteDePastas::DoPar(FolderMessage::HelperAbsent)),
        0
    );
    let enlace = ParaOAjudanteDePastas::Enlace {
        de_pe: true,
        par_suporta: true,
        nome_do_par: String::new(),
    };
    assert_eq!(numero(&enlace), 1);
    assert_eq!(
        numero(&ParaOAjudanteDePastas::Comando(ComandoDePasta::Abrir(
            PASTA
        ))),
        2
    );
}

#[test]
fn o_numero_de_cada_comando_da_janela_no_fio_nao_muda() {
    let comandos = [
        ComandoDePasta::Compartilhar {
            caminho: String::new(),
        },
        ComandoDePasta::Criar {
            nome: String::new(),
        },
        ComandoDePasta::Aceitar(PASTA),
        ComandoDePasta::Recusar(PASTA),
        ComandoDePasta::Parar(PASTA),
        ComandoDePasta::Abrir(PASTA),
        ComandoDePasta::Resolver {
            pasta: PASTA,
            caminho: String::new(),
            escolha: EscolhaDeConflito::ManterAsDuas,
        },
        ComandoDePasta::AbrirLixeira(PASTA),
        ComandoDePasta::Copiado {
            pasta: PASTA,
            caminhos: vec![],
        },
    ];
    for (esperado, comando) in comandos.iter().enumerate() {
        assert_eq!(usize::from(numero(comando)), esperado, "{comando:?}");
    }
}

#[test]
fn o_numero_de_cada_escolha_e_situacao_no_fio_nao_muda() {
    let escolhas = [
        EscolhaDeConflito::ManterAsDuas,
        EscolhaDeConflito::FicarComEsta,
        EscolhaDeConflito::FicarComAOutra,
    ];
    for (esperado, escolha) in escolhas.iter().enumerate() {
        assert_eq!(usize::from(numero(escolha)), esperado, "{escolha:?}");
    }

    let situacoes = [
        SituacaoDaPasta::EmDia,
        SituacaoDaPasta::Sincronizando,
        SituacaoDaPasta::SemConexao,
        SituacaoDaPasta::ParDesatualizado,
        SituacaoDaPasta::Oferecida,
    ];
    for (esperado, situacao) in situacoes.iter().enumerate() {
        assert_eq!(usize::from(numero(situacao)), esperado, "{situacao:?}");
    }
    assert_eq!(numero(&PapelDaPasta::Compartilhada), 0);
    assert_eq!(numero(&PapelDaPasta::Recebida), 1);
}

#[test]
fn um_bloco_cheio_do_par_cabe_numa_mensagem_do_canal_local() {
    // O canal local leva no máximo `MAX_MENSAGEM`; um trecho do par leva até um bloco cheio. Se um
    // dia o bloco crescer além do canal, o repasse quebraria no primeiro arquivo grande.
    let trecho = ParaOAjudanteDePastas::DoPar(FolderMessage::Range {
        folder: ir_proto::message::FolderId([0xff; 16]),
        request: ir_proto::message::RangeId(u32::MAX),
        offset: u64::MAX,
        data: vec![0xa5; ir_proto::limits::MAX_FILE_BLOCK],
    });
    let bytes = codec::codificar(&trecho).unwrap();
    assert!(bytes.len() <= codec::MAX_MENSAGEM + codec::PREFIXO);
}
