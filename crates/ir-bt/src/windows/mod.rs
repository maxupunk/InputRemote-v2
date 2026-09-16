//! O rádio do Windows: Winsock `AF_BTH`, a partir da sessão 0.
//!
//! O transporte precisa existir antes de haver usuário logado, para a digitação na tela de
//! login ser possível. É por isso que aqui é `AF_BTH` — uma família de sockets do kernel, sem
//! infraestrutura por usuário — e não `WinRT`
//! ([ADR-0005](../../../../docs/adr/0005-bluetooth-rfcomm-winsock.md)).
//!
//! O módulo se divide em três, e a divisão é a regra de
//! [09, §4](../../../../docs/09-padroes-de-codigo.md):
//!
//! | Módulo | O que faz | `unsafe` |
//! |---|---|---|
//! | [`winsock`] | as chamadas de FFI, embrulhadas em funções seguras | **sim**, só aqui |
//! | [`ponte`] | socket síncrono vestido de fluxo assíncrono | não |
//! | este | o [`Radio`], juntando os dois | não |

mod ponte;
mod winsock;

use tokio::sync::{Mutex, mpsc};

use self::ponte::CanalDeSocket;
use crate::addr::{BdAddr, CANAL};
use crate::error::{BtError, Result};
use crate::radio::{Dispositivo, Radio};

/// Quantas conexões o sistema enfileira enquanto não as aceitamos.
const FILA: i32 = 2;

/// Um canal que chegou: por onde ele veio, e de quem.
type Entrante = (CanalDeSocket, BdAddr);

/// O rádio Bluetooth desta máquina Windows.
#[derive(Debug)]
pub struct RadioWindows {
    entrantes: Mutex<mpsc::UnboundedReceiver<Entrante>>,
}

impl RadioWindows {
    /// Abre a escuta no canal do produto.
    ///
    /// # Errors
    ///
    /// [`BtError::SemRadio`] se não houver rádio; [`BtError::Io`] se o canal já estiver ocupado
    /// por outro programa — que o [ADR-0009] manda dizer em voz alta, em vez de sair procurando
    /// outro canal e fazer as duas pontas discordarem.
    ///
    /// [ADR-0009]: ../../../../docs/adr/0009-canal-rfcomm-fixo-sem-sdp.md
    pub fn abrir() -> Result<Self> {
        if !winsock::ha_radio() {
            return Err(BtError::SemRadio(
                "nenhum rádio Bluetooth no sistema".to_owned(),
            ));
        }
        let escuta = winsock::abrir_socket()?;
        winsock::vincular_e_escutar(escuta, CANAL, FILA)?;

        let (tx, rx) = mpsc::unbounded_channel();
        // O `accept` do Winsock é bloqueante; ele vive numa thread própria, e o runtime só vê o
        // canal.
        std::thread::spawn(move || {
            // O laço acaba quando a escuta morre (serviço encerrando, adaptador removido) ou
            // quando ninguém mais está do outro lado do canal. Insistir num socket quebrado
            // giraria esta thread sem parar.
            while let Ok((sock, origem)) = winsock::aceitar(escuta) {
                if tx.send((CanalDeSocket::novo(sock), origem)).is_err() {
                    break;
                }
            }
            winsock::fechar(escuta);
        });

        Ok(Self {
            entrantes: Mutex::new(rx),
        })
    }
}

impl Radio for RadioWindows {
    type Canal = CanalDeSocket;

    async fn disponivel(&self) -> bool {
        tokio::task::spawn_blocking(winsock::ha_radio)
            .await
            .unwrap_or(false)
    }

    async fn pareados(&self) -> Result<Vec<Dispositivo>> {
        // A enumeração não falha: sem rádio ou sem par, a resposta é uma lista vazia. O que pode
        // dar errado aqui é a própria tarefa não terminar.
        tokio::task::spawn_blocking(winsock::pareados)
            .await
            .map_err(|erro| BtError::SemRadio(erro.to_string()))
    }

    async fn conectar(&self, alvo: BdAddr) -> Result<Self::Canal> {
        // `connect` bloqueia enquanto o rádio procura o par — segundos, quando ele está longe
        // ou desligado. Numa thread do runtime isso pararia a sessão inteira.
        let ligacao = tokio::task::spawn_blocking(move || {
            let sock = winsock::abrir_socket()?;
            match winsock::conectar(sock, alvo, CANAL) {
                Ok(()) => Ok(sock),
                Err(erro) => {
                    winsock::fechar(sock);
                    Err(erro)
                }
            }
        })
        .await
        .map_err(|erro| BtError::SemRadio(erro.to_string()))?;

        match ligacao {
            Ok(sock) => Ok(CanalDeSocket::novo(sock)),
            Err(erro) => Err(traduzir(&erro, alvo)),
        }
    }

    async fn aceitar(&self) -> Result<Entrante> {
        let mut entrantes = self.entrantes.lock().await;
        match entrantes.recv().await {
            Some(entrante) => Ok(entrante),
            // A thread de escuta acabou, e ninguém mais vai ligar. Devolver erro aqui faria o
            // laço do endpoint girar sem parar; esperar é o comportamento certo de uma escuta
            // que não tem mais quem atender.
            None => std::future::pending().await,
        }
    }
}

/// Traduz a falha de conexão no que o usuário precisa ouvir.
///
/// A exigência está escrita no ADR-0005: distinguir "não pareado no sistema" de "pareado, mas o
/// serviço não responde". Um código de erro cru não faz essa distinção, e é ela que diz à pessoa
/// se o problema é dela ou nosso.
fn traduzir(erro: &std::io::Error, alvo: BdAddr) -> BtError {
    /// `WSAENETDOWN`: o rádio caiu no meio.
    const REDE_CAIU: i32 = 10050;
    /// `WSAETIMEDOUT`: o par não respondeu no prazo do rádio.
    const EXPIROU: i32 = 10060;
    /// `WSAECONNREFUSED`: alcançamos o par, e ninguém atende no canal.
    const RECUSADA: i32 = 10061;
    /// `WSAEHOSTDOWN` / `WSAEHOSTUNREACH`: o rádio não alcança o par.
    const SEM_ALCANCE: (i32, i32) = (10064, 10065);
    /// `WSAEINVAL`: o Windows recusa conectar a quem não tem vínculo gravado.
    const INVALIDO: i32 = 10022;

    match erro.raw_os_error() {
        Some(RECUSADA | EXPIROU) => BtError::SemResposta,
        Some(codigo) if codigo == SEM_ALCANCE.0 || codigo == SEM_ALCANCE.1 => BtError::SemResposta,
        Some(INVALIDO) => BtError::NaoPareado(alvo.to_string()),
        Some(REDE_CAIU) => BtError::SemRadio("o rádio Bluetooth caiu".to_owned()),
        _ => BtError::Io(std::io::Error::new(erro.kind(), erro.to_string())),
    }
}
