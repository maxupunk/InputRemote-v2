//! A janela reconecta sozinha — conduzido contra um serviço de mentira, sem sistema operacional.
//!
//! O serviço destes testes fala o mesmo protocolo do de verdade, num socket TCP de *loopback*,
//! porque é o duplex que existe igual em Windows e Linux na biblioteca padrão. O que se prova não
//! é o transporte — é a vida da ligação: nascer sem serviço, conectar quando ele sobe, perceber
//! quando ele cai e voltar, e distinguir "parado" de "sem permissão".

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ir_ipc::codec::{self, PREFIXO};
use ir_ipc::{Estado, Falha, Maquina, Nome, ParaInterface, Pedido, Resposta};
use ir_ui::conector::{Conector, Duplex};
use ir_ui::real::ServicoReal;
use ir_ui::servico::{Desconexao, Servico, Situacao};

/// Teto de voltas nas esperas por algo que acontece em outra thread. Finito: um teste que gira
/// para sempre por causa de um defeito é pior que um que falha.
const TETO: u32 = 300;

/// Um serviço de mentira, que aceita conexões e responde pedidos como o de verdade.
struct ServicoDeMentira {
    endereco: SocketAddr,
    /// As conexões abertas, para o teste poder derrubá-las como um serviço que reinicia.
    abertas: Arc<Mutex<Vec<TcpStream>>>,
    /// Quantas conexões já foram aceitas.
    aceitas: Arc<AtomicUsize>,
    /// Se está recusando por permissão.
    recusando: Arc<AtomicBool>,
}

impl ServicoDeMentira {
    fn subir() -> Self {
        let ouvinte = TcpListener::bind("127.0.0.1:0").expect("vincula");
        let endereco = ouvinte.local_addr().expect("endereço");
        let abertas = Arc::new(Mutex::new(Vec::new()));
        let aceitas = Arc::new(AtomicUsize::new(0));
        let recusando = Arc::new(AtomicBool::new(false));

        let (lista, contagem, recusa) = (
            Arc::clone(&abertas),
            Arc::clone(&aceitas),
            Arc::clone(&recusando),
        );
        std::thread::spawn(move || {
            for conexao in ouvinte.incoming().map_while(Result::ok) {
                contagem.fetch_add(1, Ordering::SeqCst);
                lista.lock().unwrap().push(conexao.try_clone().unwrap());
                let recusa = Arc::clone(&recusa);
                std::thread::spawn(move || atender(conexao, &recusa));
            }
        });
        Self {
            endereco,
            abertas,
            aceitas,
            recusando,
        }
    }

    /// Derruba todas as conexões abertas, como um serviço que reinicia.
    fn derrubar_conexoes(&self) {
        for conexao in self.abertas.lock().unwrap().drain(..) {
            let _ = conexao.shutdown(Shutdown::Both);
        }
    }

    fn aceitas(&self) -> usize {
        self.aceitas.load(Ordering::SeqCst)
    }
}

/// Atende uma conexão: recusa por permissão, ou responde cada pedido.
fn atender(mut conexao: TcpStream, recusando: &AtomicBool) {
    while let Some(pedido) = ler_pedido(&mut conexao) {
        if recusando.load(Ordering::SeqCst) {
            // Como o serviço de verdade: a recusa responde ao primeiro pedido, e o canal fecha.
            escrever(&mut conexao, &Resposta::Falha(Falha::SemPermissao));
            return;
        }
        let resposta = match pedido {
            Pedido::Estado => Resposta::Estado(Estado::recem_instalado(
                Maquina([7; 16]),
                Nome::coagido("bancada"),
            )),
            _ => Resposta::Feito,
        };
        escrever(&mut conexao, &resposta);
    }
}

fn escrever(conexao: &mut TcpStream, resposta: &Resposta) {
    let quadro = codec::codificar(&ParaInterface::Resposta(resposta.clone())).unwrap();
    let _ = conexao.write_all(&quadro);
}

fn ler_pedido(conexao: &mut TcpStream) -> Option<Pedido> {
    let mut prefixo = [0u8; PREFIXO];
    conexao.read_exact(&mut prefixo).ok()?;
    let tamanho = codec::tamanho_anunciado(&prefixo).ok()?;
    let mut corpo = vec![0u8; tamanho];
    conexao.read_exact(&mut corpo).ok()?;
    codec::decodificar(&corpo).ok()
}

/// Um conector que o teste liga e desliga: sem endereço, o serviço "não está no ar".
#[derive(Clone, Default)]
struct ConectorDeTeste {
    endereco: Arc<Mutex<Option<SocketAddr>>>,
}

impl ConectorDeTeste {
    fn apontar(&self, endereco: SocketAddr) {
        *self.endereco.lock().unwrap() = Some(endereco);
    }
}

impl Conector for ConectorDeTeste {
    fn abrir(&self) -> std::io::Result<Duplex> {
        let Some(endereco) = *self.endereco.lock().unwrap() else {
            return Err(std::io::ErrorKind::NotFound.into());
        };
        let escrita = TcpStream::connect(endereco)?;
        let leitura = escrita.try_clone()?;
        Ok((Box::new(escrita), Box::new(leitura)))
    }
}

/// Um serviço real da interface, sem espera entre tentativas para o teste não dormir.
fn interface(conector: &ConectorDeTeste) -> ServicoReal {
    ServicoReal::com(Box::new(conector.clone()), Duration::ZERO)
}

/// Deixa a interface fazer a consulta periódica até o critério valer.
fn ate(servico: &ServicoReal, criterio: impl Fn(&ServicoReal) -> bool) {
    for _ in 0..TETO {
        let _ = servico.avisos();
        if criterio(servico) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "o critério não se cumpriu; situação: {:?}",
        servico.situacao()
    );
}

#[test]
fn sem_servico_a_janela_diz_que_ele_esta_parado_e_o_pedido_falha_explicado() {
    let servico = interface(&ConectorDeTeste::default());

    assert_eq!(
        servico.situacao(),
        Situacao::Desconectado(Desconexao::ServicoParado)
    );
    // Falha com razão e instrução, e não "falha interna": quem olha precisa saber o que fazer.
    assert_eq!(
        servico.pedir(Pedido::Estado),
        Resposta::Falha(Falha::ServicoIndisponivel)
    );
}

#[test]
fn quando_o_servico_sobe_a_janela_conecta_sozinha() {
    let conector = ConectorDeTeste::default();
    let servico = interface(&conector);
    assert_ne!(servico.situacao(), Situacao::Conectado);

    // O serviço sobe depois de a janela já estar aberta — o caso que antes a prendia no simulado.
    let mentira = ServicoDeMentira::subir();
    conector.apontar(mentira.endereco);
    ate(&servico, |servico| {
        servico.situacao() == Situacao::Conectado
    });

    assert!(matches!(servico.pedir(Pedido::Estado), Resposta::Estado(_)));
}

#[test]
fn quando_o_servico_cai_a_janela_percebe_e_volta() {
    let mentira = ServicoDeMentira::subir();
    let conector = ConectorDeTeste::default();
    conector.apontar(mentira.endereco);
    let servico = interface(&conector);
    assert_eq!(servico.situacao(), Situacao::Conectado);
    assert_eq!(mentira.aceitas(), 1);

    // O serviço reinicia: a conexão cai. A janela precisa abrir outra sem ninguém mandar.
    mentira.derrubar_conexoes();
    ate(&servico, |_| mentira.aceitas() >= 2);
    ate(&servico, |servico| {
        servico.situacao() == Situacao::Conectado
    });

    assert!(
        matches!(servico.pedir(Pedido::Estado), Resposta::Estado(_)),
        "depois de voltar, os pedidos precisam funcionar de novo"
    );
}

#[test]
fn sem_permissao_a_janela_diz_o_que_fazer_e_entra_assim_que_a_permissao_e_dada() {
    let mentira = ServicoDeMentira::subir();
    mentira.recusando.store(true, Ordering::SeqCst);
    let conector = ConectorDeTeste::default();
    conector.apontar(mentira.endereco);

    let servico = interface(&conector);
    // "Sem permissão" e "serviço parado" pedem ações sem nada em comum; confundir os dois manda a
    // pessoa investigar o problema errado.
    assert_eq!(
        servico.situacao(),
        Situacao::Desconectado(Desconexao::SemPermissao)
    );

    // O administrador libera o usuário. Sem reiniciar nada, a janela entra na tentativa seguinte.
    mentira.recusando.store(false, Ordering::SeqCst);
    ate(&servico, |servico| {
        servico.situacao() == Situacao::Conectado
    });
}
