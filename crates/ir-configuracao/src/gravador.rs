//! As gravações da configuração, numa thread própria e **em ordem**.
//!
//! Gravar é escrever um arquivo temporário, `sync_all` e renomear — num disco lento, dezenas de
//! milissegundos. O laço do serviço (`ir-daemon`) bate a cada 5 ms com o controle no par, e três gravações
//! acontecem justamente com a sessão de pé: o endereço do rádio do par, o nome dele e a borda que o
//! servidor anunciou. Feitas ali, elas eram um tranco no ponteiro logo depois de conectar.
//!
//! Todas passam por esta fila, inclusive as que esperam o resultado: se uma gravação sem espera
//! ficasse para trás de uma com espera, a mais velha terminaria por último e apagaria a mais nova.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use tracing::warn;

use crate::Config;

/// Um pedido de gravação: o que gravar e, se alguém espera, por onde responder.
struct Pedido {
    /// Sem configuração é só uma barreira: responde quando o que veio antes já está no disco.
    config: Option<Config>,
    resposta: Option<Sender<anyhow::Result<()>>>,
}

/// A fila de gravação da configuração.
#[derive(Debug, Clone)]
pub struct Gravador {
    fila: Sender<Pedido>,
    pasta: PathBuf,
}

impl Gravador {
    /// Sobe a thread que grava em `pasta`.
    #[must_use]
    pub fn novo(pasta: PathBuf) -> Self {
        let (fila, chegam) = mpsc::channel();
        let destino = pasta.clone();
        let criada = thread::Builder::new()
            .name("gravador".to_owned())
            .spawn(move || gravar_para_sempre(&chegam, &destino));
        if let Err(erro) = criada {
            // Sem a thread, o canal fica fechado e cada gravação é feita ali mesmo, como antes.
            warn!(%erro, "a thread de gravação da configuração não subiu");
        }
        Self { fila, pasta }
    }

    /// Grava sem esperar. Uma falha fica no registro: quem pede já adotou o valor na memória.
    pub fn gravar(&self, config: &Config) {
        let pedido = Pedido {
            config: Some(config.clone()),
            resposta: None,
        };
        if self.fila.send(pedido).is_err() {
            warn!("sem a thread de gravação; gravando aqui mesmo");
            if let Err(erro) = config.save(&self.pasta) {
                warn!(%erro, "não foi possível gravar a configuração");
            }
        }
    }

    /// Grava e espera o resultado — para o que a janela precisa saber se ficou gravado.
    ///
    /// # Errors
    ///
    /// Os da gravação.
    pub fn gravar_e_esperar(&self, config: &Config) -> anyhow::Result<()> {
        let (resposta, volta) = mpsc::channel();
        let pedido = Pedido {
            config: Some(config.clone()),
            resposta: Some(resposta),
        };
        if self.fila.send(pedido).is_err() {
            return config.save(&self.pasta);
        }
        volta
            .recv()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("a thread de gravação caiu")))
    }

    /// Espera tudo o que já foi pedido chegar ao disco.
    pub fn esperar(&self) {
        let (resposta, volta) = mpsc::channel();
        // A fila é em ordem: quando a resposta deste chega, os anteriores já foram gravados.
        let barreira = Pedido {
            config: None,
            resposta: Some(resposta),
        };
        if self.fila.send(barreira).is_ok() {
            let _ = volta.recv();
        }
    }
}

/// O laço da thread: grava cada pedido na ordem em que chegou.
fn gravar_para_sempre(chegam: &Receiver<Pedido>, pasta: &std::path::Path) {
    while let Ok(pedido) = chegam.recv() {
        let resultado = pedido.config.map_or(Ok(()), |config| config.save(pasta));
        match pedido.resposta {
            Some(resposta) => {
                let _ = resposta.send(resultado);
            }
            None => {
                if let Err(erro) = resultado {
                    warn!(%erro, "não foi possível gravar a configuração");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_ultima_gravacao_e_a_que_fica_mesmo_misturando_espera_e_nao_espera() {
        let pasta = std::env::temp_dir().join(format!("ir-gravador-{}", std::process::id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let gravador = Gravador::novo(pasta.clone());
        let mut config = Config::default();
        for porta in 1..=20 {
            config.port = porta;
            gravador.gravar(&config);
        }
        config.port = 4242;
        gravador.gravar_e_esperar(&config).unwrap();
        assert_eq!(crate::load_config(&pasta).unwrap().port, 4242);
        let _ = std::fs::remove_dir_all(pasta);
    }
}
