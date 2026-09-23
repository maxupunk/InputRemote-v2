//! O canal do agente: recebe [`FatoDoAgente`] e empurra [`ComandoDoAgente`].
//!
//! É o **segundo** transporte, separado do da interface de propósito. A interface nunca pode
//! pedir injeção de entrada: se qualquer processo do usuário pudesse mandar `Injetar` para o
//! serviço, qualquer programa que ele rodasse poderia digitar no prompt de UAC
//! ([04, §5](../../../docs/04-seguranca.md)). Dois canais, dois vocabulários que não se
//! misturam — e é isso que torna a garantia estrutural, e não combinada.
//!
//! Como no canal de controle, aqui só se movem quadros: o agente não decide nada, e toda a
//! política vive no ator.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ir_ipc::{ComandoDoAgente, FatoDoAgente};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info, warn};

use super::escuta::{Chamada, Conexao, Escuta};
use super::quadros;

/// Aceita o agente — **um de cada vez**.
///
/// A exclusão é essencial, não higiene: o serviço relança o agente enquanto não houver um de pé,
/// e o agente leva um instante para conectar depois de lançado. Sem esta trava, dois agentes
/// poderiam capturar a mesma sessão ao mesmo tempo, e cada tecla chegaria duplicada ao par.
pub(crate) async fn servir(
    mut escuta: Escuta,
    fatos: UnboundedSender<FatoDoAgente>,
    comandos: broadcast::Sender<ComandoDoAgente>,
) {
    let ocupado = Arc::new(AtomicBool::new(false));
    loop {
        match escuta.aceitar().await {
            Ok((conexao, Chamada::Negada { uid })) => {
                // Um processo que não é o serviço tentando abrir o canal que carrega injeção de
                // entrada. Não há o que explicar a ele: fecha, e fica registrado.
                warn!(
                    uid,
                    "um processo sem permissão tentou abrir o canal do agente; recusado"
                );
                drop(conexao);
            }
            Ok((conexao, Chamada::Permitida)) => {
                if ocupado.swap(true, Ordering::SeqCst) {
                    // Já há agente servindo. Fechar na cara é o certo: o segundo percebe o fim
                    // do fluxo e sai sozinho, antes de instalar gancho nenhum.
                    warn!("um segundo agente tentou conectar; recusando");
                    drop(conexao);
                    continue;
                }
                info!("agente conectado");
                let fatos = fatos.clone();
                let comandos = comandos.subscribe();
                tokio::spawn(atender(conexao, fatos, comandos, Arc::clone(&ocupado)));
            }
            Err(erro) => {
                warn!(%erro, "falha ao aceitar o agente");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
}

/// Atende o agente até a conexão cair.
async fn atender(
    conexao: Conexao,
    fatos: UnboundedSender<FatoDoAgente>,
    mut comandos: broadcast::Receiver<ComandoDoAgente>,
    ocupado: Arc<AtomicBool>,
) {
    let (mut leitura, mut escrita) = tokio::io::split(conexao);
    loop {
        tokio::select! {
            quadro = quadros::ler::<_, FatoDoAgente>(&mut leitura) => {
                match quadro {
                    Ok(Some(fato)) => {
                        if fatos.send(fato).is_err() {
                            break; // o ator saiu
                        }
                    }
                    Ok(None) => break,
                    Err(erro) => {
                        warn!(%erro, "quadro do agente malformado; encerrando conexão");
                        break;
                    }
                }
            }
            comando = comandos.recv() => {
                match comando {
                    Ok(comando) => {
                        if quadros::escrever(&mut escrita, &comando).await.is_err() {
                            break;
                        }
                    }
                    // Perder um comando de injeção é perder uma tecla — pode ser justamente a
                    // subida de uma. Seguir adiante deixaria a tecla presa; derrubar a conexão faz
                    // o agente soltar tudo ao sair, e a sessão soltar o que for dela
                    // ([02, §6](../../../docs/02-arquitetura.md): a fila cheia derruba o enlace,
                    // não perde tecla).
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(perdidos = n, "o agente não acompanhou os comandos; derrubando a conexão para soltar tudo");
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    // Libera a vaga **antes** de avisar, para o próximo agente já poder entrar. Só quem chegou a
    // ocupar a vaga anuncia o encerramento: uma conexão recusada não pode derrubar o agente bom.
    ocupado.store(false, Ordering::SeqCst);
    let _ = fatos.send(FatoDoAgente::Encerrou);
    debug!("conexão do agente encerrada");
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;

    /// Um endereço de teste único para esta execução, para dois testes não colidirem.
    fn endereco_de_teste(rotulo: &str) -> String {
        let id = std::process::id();
        #[cfg(windows)]
        {
            format!(r"\\.\pipe\inputremote-test-{rotulo}-{id}")
        }
        #[cfg(not(windows))]
        {
            std::env::temp_dir()
                .join(format!("ir-test-{rotulo}-{id}.sock"))
                .to_string_lossy()
                .into_owned()
        }
    }

    #[cfg(windows)]
    async fn conectar_agente(
        endereco: &str,
    ) -> impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin {
        use tokio::net::windows::named_pipe::ClientOptions;
        for _ in 0..50 {
            if let Ok(cliente) = ClientOptions::new().open(endereco) {
                return cliente;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("o agente de teste não conectou");
    }

    #[cfg(not(windows))]
    async fn conectar_agente(
        endereco: &str,
    ) -> impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin {
        for _ in 0..50 {
            if let Ok(cliente) = tokio::net::UnixStream::connect(endereco).await {
                return cliente;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("o agente de teste não conectou");
    }

    #[tokio::test]
    async fn o_fato_sobe_o_comando_desce_e_o_segundo_agente_e_recusado() {
        let endereco = endereco_de_teste("agente");
        // O canal de verdade nasce `Restrito`, e aí só o serviço o alcança — nem este teste,
        // que roda como usuário comum, conseguiria abrir. Quem é exercitado aqui é a **lógica**
        // do canal (fato sobe, comando desce, segundo agente recusado); o descritor de segurança
        // é assunto de `ir_acesso::seguranca`, e o teste do canal de controle já prova que a
        // permissão declarada é a que vale.
        let escuta = Escuta::abrir(&endereco, crate::escuta::Acesso::UsuarioInterativo)
            .expect("abre o ponto de escuta");
        let (fato_tx, mut fatos) = mpsc::unbounded_channel();
        let (comandos, _) = broadcast::channel(16);
        tokio::spawn(servir(escuta, fato_tx, comandos.clone()));

        let cliente = conectar_agente(&endereco).await;
        let (mut leitura, mut escrita) = tokio::io::split(cliente);

        // O fato do agente sobe até o ator.
        let pronto = FatoDoAgente::Pronto {
            desktops: vec!["Default".to_owned()],
        };
        quadros::escrever(&mut escrita, &pronto)
            .await
            .expect("envia fato");
        assert_eq!(fatos.recv().await.expect("fato chega"), pronto);

        // E o comando do ator desce até o agente.
        comandos
            .send(ComandoDoAgente::SoltarTudo)
            .expect("há assinante");
        let comando: ComandoDoAgente = quadros::ler(&mut leitura)
            .await
            .expect("lê comando")
            .expect("comando presente");
        assert_eq!(comando, ComandoDoAgente::SoltarTudo);

        // Um segundo agente é fechado na hora: dois capturando a mesma sessão duplicariam cada
        // tecla, e o serviço relança o agente enquanto não houver um de pé.
        let segundo = conectar_agente(&endereco).await;
        let (mut leitura_do_segundo, _escrita) = tokio::io::split(segundo);
        let fim: Option<ComandoDoAgente> = quadros::ler(&mut leitura_do_segundo)
            .await
            .expect("lê o fim do fluxo");
        assert!(fim.is_none(), "o segundo agente devia ter sido recusado");
    }
}
