//! Cliente das pastas compartilhadas sem interface gráfica, para a bancada.
//!
//! ```text
//! pastas <endereco> listar
//! pastas <endereco> compartilhar C:\caminho\da\pasta
//! pastas <endereco> criar "Nome da pasta"
//! pastas <endereco> aceitar          # aceita a primeira oferta, se houver
//! pastas <endereco> parar <posicao>  # para de compartilhar a pasta desta posição da lista
//! pastas <endereco> copiar <posicao> <relativo>  # o Ctrl+C de dentro da pasta, como o ajudante faz
//! pastas <endereco> clipboard        # espera o que o outro computador copiou da pasta (20 s)
//! ```
//!
//! Fala pelo canal de controle, como a janela: o serviço repassa ao ajudante das pastas. Não usa
//! `unwrap`, `expect` nem `panic`: um exemplo não é teste.

use std::io::{Read, Write};

use ir_ipc::pastas::{ComandoDePasta, ResumoDePasta, SituacaoDaPasta};
use ir_ipc::{ParaInterface, Pedido, Resposta, codec};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(endereco), Some(acao)) = (args.next(), args.next()) else {
        return println!(
            "uso: pastas <endereco> listar|compartilhar <caminho>|criar <nome>|aceitar|parar <posicao>|copiar <posicao> <relativo>|clipboard"
        );
    };
    let argumento = args.next();
    let (mut escrita, mut leitura) = match ir_ipc::cliente::abrir(&endereco) {
        Ok(duplex) => duplex,
        Err(erro) => return println!("ERRO: não abriu {endereco}: {erro}"),
    };
    let lista = pedir(&mut escrita, &mut leitura, &Pedido::Pastas);
    let lista = match lista {
        Some(Resposta::Pastas(lista)) => lista,
        outra => return println!("ERRO: resposta inesperada: {outra:?}"),
    };
    let comando = match (acao.as_str(), argumento) {
        ("listar", _) => return mostrar(&lista),
        ("clipboard", _) => return esperar_clipboard(&mut escrita, &mut leitura),
        ("copiar", Some(posicao)) => {
            match posicao.parse::<usize>().ok().and_then(|i| lista.get(i)) {
                Some(pasta) => ComandoDePasta::Copiado {
                    pasta: pasta.id,
                    caminhos: args.collect(),
                },
                None => return println!("ERRO: não há pasta nessa posição"),
            }
        }
        ("compartilhar", Some(caminho)) => ComandoDePasta::Compartilhar { caminho },
        ("criar", Some(nome)) => ComandoDePasta::Criar { nome },
        ("aceitar", _) => {
            let oferta = lista
                .iter()
                .find(|p| p.situacao == SituacaoDaPasta::Oferecida && p.caminho_local.is_empty());
            match oferta {
                Some(oferta) => ComandoDePasta::Aceitar(oferta.id),
                None => return println!("nenhuma oferta esperando"),
            }
        }
        ("parar", Some(posicao)) => {
            match posicao.parse::<usize>().ok().and_then(|i| lista.get(i)) {
                Some(pasta) => ComandoDePasta::Parar(pasta.id),
                None => return println!("ERRO: não há pasta nessa posição"),
            }
        }
        (outra, _) => return println!("ERRO: ação desconhecida ou sem argumento: {outra}"),
    };
    match pedir(&mut escrita, &mut leitura, &Pedido::Pasta(comando)) {
        Some(Resposta::Feito) => println!("FEITO"),
        outra => println!("RESPOSTA: {outra:?}"),
    }
}

/// Escuta como o ajudante de clipboard escuta, e mostra o que iria para o clipboard.
fn esperar_clipboard(escrita: &mut Box<dyn Write + Send>, leitura: &mut Box<dyn Read + Send>) {
    if codec::escrever_em(escrita, &Pedido::AcompanharClipboard).is_err() {
        return println!("ERRO: não escreveu");
    }
    let fim = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < fim {
        match codec::ler_de::<ParaInterface, _>(leitura) {
            Ok(Some(ParaInterface::Aviso(ir_ipc::Aviso::ArquivosDaPasta(caminhos)))) => {
                for caminho in caminhos {
                    println!("NO CLIPBOARD: {caminho}");
                }
                return;
            }
            Ok(Some(_)) => {}
            _ => return println!("ERRO: o canal caiu"),
        }
    }
    println!("nada chegou em 20 s");
}

fn mostrar(lista: &[ResumoDePasta]) {
    if lista.is_empty() {
        return println!("nenhuma pasta");
    }
    for (posicao, pasta) in lista.iter().enumerate() {
        println!(
            "[{posicao}] {} — {:?} {:?} pendentes={} conflitos={}\n    {}\n    {}",
            pasta.nome,
            pasta.papel,
            pasta.situacao,
            pasta.pendentes,
            pasta.conflitos,
            pasta.linha(),
            pasta.detalhe()
        );
    }
}

/// Manda um pedido e devolve a resposta, pulando os avisos que chegarem no meio.
fn pedir(
    escrita: &mut Box<dyn Write + Send>,
    leitura: &mut Box<dyn Read + Send>,
    pedido: &Pedido,
) -> Option<Resposta> {
    if let Err(erro) = codec::escrever_em(escrita, pedido) {
        println!("ERRO: não escreveu: {erro}");
        return None;
    }
    for _ in 0..100 {
        if let ParaInterface::Resposta(resposta) = codec::ler_de(leitura).ok().flatten()? {
            return Some(resposta);
        }
    }
    None
}
