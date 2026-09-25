//! A vida da ligação com o serviço: abrir, perceber que caiu, e abrir de novo.
//!
//! Antes, a interface escolhia uma vez só, ao abrir, entre o serviço de verdade e o simulado — e
//! ninguém cuidava da conexão depois disso. Se o serviço reiniciava (uma atualização de pacote,
//! uma queda), a janela ficava sem conexão até ser fechada; se ela abria com o serviço parado,
//! caía no simulado e nunca mais tentava. Este módulo é o dono dessa responsabilidade, e só dela:
//! ele não sabe abrir canal nenhum ([`Conector`]) nem o que a janela mostra ([`Situacao`]).
//!
//! # Como o duplex vira pedido-resposta mais avisos
//!
//! O canal carrega dois tipos de mensagem misturados: a resposta a um pedido e um aviso que o
//! serviço manda por conta própria ([`ParaInterface`]). Uma thread de leitura separa os dois:
//! respostas vão para um canal que [`Conexao::pedir`] espera; avisos entram numa fila que
//! [`Conexao::avisos`] esvazia. Quando o serviço fecha o canal, a mesma thread marca a ligação
//! como morta, e a próxima consulta tenta de novo. Tudo bloqueante e de biblioteca padrão: a
//! interface roda numa thread só, e o Slint não é `tokio`.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ir_ipc::codec;
use ir_ipc::{Aviso, Falha, ParaInterface, Pedido, Resposta};

use crate::conector::Conector;
use crate::servico::{Desconexao, Situacao};

/// De quanto em quanto tempo se tenta de novo quando o serviço não está no ar.
///
/// Um segundo: abrir um canal local que não existe custa quase nada, e é o bastante para a janela
/// voltar a funcionar sem que ninguém perceba o intervalo.
pub const INTERVALO_DE_RECONEXAO: Duration = Duration::from_secs(1);

/// Quantas vezes o intervalo normal se espera quando a recusa é de permissão.
///
/// Permissão muda na velocidade de uma pessoa digitando um comando, não na de um serviço
/// reiniciando — e cada tentativa recusada vira uma linha no registro do serviço.
const FATOR_SEM_PERMISSAO: u32 = 10;

/// Quanto tempo um pedido espera pela resposta antes de desistir.
///
/// Todo pedido é local e curto; um que passe disto é sinal de serviço travado, e travar a janela
/// junto seria a pior resposta possível.
const ESPERA: Duration = Duration::from_secs(2);

/// A fila de avisos, compartilhada com a thread de leitura.
type Fila = Arc<Mutex<VecDeque<Aviso>>>;

/// Um canal aberto: por onde se escreve, por onde chegam as respostas, e se ele ainda vive.
///
/// Escrita e respostas ficam juntas para serializar a chamada: um pedido escreve e então espera a
/// resposta dele, sem que outro pedido se enfie no meio e receba a resposta trocada.
struct Canal {
    escrita: Box<dyn Write + Send>,
    respostas: Receiver<Resposta>,
    vivo: Arc<AtomicBool>,
}

/// A ligação com o serviço, que se refaz sozinha.
pub struct Conexao {
    conector: Box<dyn Conector>,
    intervalo: Duration,
    canal: Option<Canal>,
    ultima_tentativa: Option<Instant>,
    situacao: Situacao,
    avisos: Fila,
}

impl std::fmt::Debug for Conexao {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conexao")
            .field("situacao", &self.situacao)
            .field("intervalo", &self.intervalo)
            .finish_non_exhaustive()
    }
}

impl Conexao {
    /// Uma ligação que usa `conector` e tenta de novo a cada `intervalo`.
    ///
    /// Já tenta uma vez aqui, para a janela nascer mostrando a situação real, e não um "tentando"
    /// que dura até a primeira batida.
    #[must_use]
    pub fn new(conector: Box<dyn Conector>, intervalo: Duration) -> Self {
        let mut conexao = Self {
            conector,
            intervalo,
            canal: None,
            ultima_tentativa: None,
            situacao: Situacao::Desconectado(Desconexao::ServicoParado),
            avisos: Fila::default(),
        };
        conexao.garantir();
        conexao
    }

    /// Em que pé está a ligação, agora.
    #[must_use]
    pub const fn situacao(&self) -> Situacao {
        self.situacao
    }

    /// Percebe se a ligação caiu e, se for hora, tenta de novo.
    pub fn garantir(&mut self) {
        if self
            .canal
            .as_ref()
            .is_some_and(|canal| !canal.vivo.load(Ordering::Relaxed))
        {
            self.cair();
        }
        if self.canal.is_none() && self.hora_de_tentar() {
            self.tentar();
        }
    }

    /// Tenta de novo agora, sem esperar o intervalo.
    ///
    /// Para quando a janela sabe que algo mudou por ação dela — a ativação acabou de ligar o serviço
    /// ou liberar o acesso. Esperar os dez segundos da recusa de permissão, nesse momento, faria a
    /// pessoa achar que a senha não adiantou.
    pub fn tentar_agora(&mut self) {
        self.ultima_tentativa = None;
        self.garantir();
    }

    /// Faz um pedido. Sem ligação, falha com a razão em vez de travar ou mentir.
    pub fn pedir(&mut self, pedido: &Pedido) -> Resposta {
        self.garantir();
        let Some(canal) = self.canal.as_mut() else {
            return Resposta::Falha(Falha::ServicoIndisponivel);
        };
        if let Some(resposta) = trocar(canal, pedido) {
            return resposta;
        }
        // Escrita recusada ou resposta que não veio: o canal não serve mais. Descartá-lo também
        // impede que uma resposta atrasada seja entregue ao pedido seguinte.
        self.cair();
        Resposta::Falha(Falha::ServicoIndisponivel)
    }

    /// Recolhe os avisos que chegaram — e aproveita a consulta periódica para manter a ligação.
    pub fn avisos(&mut self) -> Vec<Aviso> {
        self.garantir();
        self.avisos
            .lock()
            .map(|mut fila| fila.drain(..).collect())
            .unwrap_or_default()
    }

    /// Se já passou tempo bastante desde a última tentativa.
    fn hora_de_tentar(&self) -> bool {
        let espera = match self.situacao {
            Situacao::Desconectado(Desconexao::SemPermissao) => {
                self.intervalo.saturating_mul(FATOR_SEM_PERMISSAO)
            }
            _ => self.intervalo,
        };
        self.ultima_tentativa
            .is_none_or(|quando| quando.elapsed() >= espera)
    }

    /// A ligação caiu.
    fn cair(&mut self) {
        self.canal = None;
        self.situacao = Situacao::Desconectado(Desconexao::ServicoParado);
    }

    /// Abre um canal novo e faz o aperto de mão.
    fn tentar(&mut self) {
        self.ultima_tentativa = Some(Instant::now());
        let (escrita, leitura) = match self.conector.abrir() {
            Ok(duplex) => duplex,
            Err(erro) => {
                self.situacao = Situacao::Desconectado(desconexao_de(&erro));
                return;
            }
        };
        let vivo = Arc::new(AtomicBool::new(true));
        let (respostas_tx, respostas) = std::sync::mpsc::channel();
        iniciar_leitor(
            leitura,
            respostas_tx,
            Arc::clone(&self.avisos),
            Arc::clone(&vivo),
        );
        let mut canal = Canal {
            escrita,
            respostas,
            vivo,
        };
        // `Acompanhar` é o aperto de mão: sem ele o serviço não empurra avisos, e o código de
        // pareamento nunca chegaria à tela. A resposta também diz se esta ligação foi aceita.
        self.situacao = match trocar(&mut canal, &Pedido::Acompanhar) {
            Some(Resposta::Falha(Falha::SemPermissao)) => {
                Situacao::Desconectado(Desconexao::SemPermissao)
            }
            Some(_) => {
                self.canal = Some(canal);
                Situacao::Conectado
            }
            None => Situacao::Desconectado(Desconexao::ServicoParado),
        };
    }
}

/// Escreve um pedido e espera a resposta dele. `None` se o canal não serve mais.
fn trocar(canal: &mut Canal, pedido: &Pedido) -> Option<Resposta> {
    codec::escrever_em(&mut canal.escrita, pedido).ok()?;
    canal.respostas.recv_timeout(ESPERA).ok()
}

/// O motivo, na linguagem da janela, de o canal não ter aberto.
fn desconexao_de(erro: &std::io::Error) -> Desconexao {
    if erro.kind() == std::io::ErrorKind::PermissionDenied {
        Desconexao::SemPermissao
    } else {
        Desconexao::ServicoParado
    }
}

/// A thread que lê o canal, separa respostas de avisos e marca a ligação como morta ao terminar.
fn iniciar_leitor(
    mut leitura: Box<dyn Read + Send>,
    respostas: Sender<Resposta>,
    avisos: Fila,
    vivo: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        loop {
            match codec::ler_de::<ParaInterface, _>(&mut leitura) {
                Ok(Some(ParaInterface::Resposta(resposta))) => {
                    if respostas.send(resposta).is_err() {
                        break;
                    }
                }
                Ok(Some(ParaInterface::Aviso(aviso))) => {
                    if let Ok(mut fila) = avisos.lock() {
                        fila.push_back(aviso);
                    }
                }
                // Variante futura do envelope, ainda não conhecida por esta versão: ignorar.
                Ok(Some(_)) => {}
                // Fim de fluxo limpo ou erro de leitura: o serviço fechou o canal.
                Ok(None) | Err(_) => break,
            }
        }
        vivo.store(false, Ordering::Relaxed);
    });
}
