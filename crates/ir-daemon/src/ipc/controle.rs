//! O canal de controle: serve [`Pedido`] e empurra [`Aviso`] para cada interface conectada.
//!
//! Uma tarefa por conexão. Ela lê pedidos, encaminha ao ator (o único dono do estado) por um
//! canal, espera a resposta e a devolve; e, em paralelo, repassa os avisos que o ator emite.
//! A tradução entre o estado interno e o [`ir_ipc::Estado`] publicado acontece no ator, não
//! aqui — este módulo só move bytes.

use ir_ipc::{Aviso, ParaInterface, Pedido};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use super::escuta::{Conexao, Escuta};
use super::{PedidoRecebido, quadros};

/// Aceita conexões para sempre, uma tarefa por cliente.
pub(crate) async fn servir(
    mut escuta: Escuta,
    pedidos: UnboundedSender<PedidoRecebido>,
    avisos: broadcast::Sender<Aviso>,
) {
    loop {
        match escuta.aceitar().await {
            Ok(conexao) => {
                // Registrado em nível alto de propósito: é como se confirma, no diagnóstico, que
                // a janela achou o serviço em vez de ter caído para o simulado em silêncio.
                info!("interface conectada");
                let pedidos = pedidos.clone();
                let avisos = avisos.subscribe();
                tokio::spawn(atender(conexao, pedidos, avisos));
            }
            Err(erro) => {
                warn!(%erro, "falha ao aceitar conexão de controle");
                // Um erro de aceitação não derruba o serviço; espera e tenta de novo.
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
}

/// Atende uma conexão até ela fechar.
async fn atender(
    conexao: Conexao,
    pedidos: UnboundedSender<PedidoRecebido>,
    mut avisos: broadcast::Receiver<Aviso>,
) {
    let (mut leitura, mut escrita) = tokio::io::split(conexao);
    // Só depois de a interface pedir para acompanhar é que os avisos começam a fluir; antes
    // disso ela recebe só as respostas aos próprios pedidos.
    let mut acompanhando = false;
    loop {
        tokio::select! {
            quadro = quadros::ler::<_, Pedido>(&mut leitura) => {
                match quadro {
                    Ok(Some(pedido)) => {
                        if matches!(pedido, Pedido::Acompanhar) {
                            acompanhando = true;
                        }
                        if !responder(&pedidos, &mut escrita, pedido).await {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(erro) => {
                        warn!(%erro, "quadro de controle malformado; encerrando conexão");
                        break;
                    }
                }
            }
            aviso = avisos.recv(), if acompanhando => {
                match aviso {
                    Ok(aviso) => {
                        let msg = ParaInterface::Aviso(aviso);
                        if quadros::escrever(&mut escrita, &msg).await.is_err() {
                            break;
                        }
                    }
                    // Uma interface lenta pode perder avisos; ela se reconcilia pelo próximo
                    // estado, então um atraso não é motivo para derrubar a conexão.
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(perdidos = n, "interface atrasada perdeu avisos");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    debug!("conexão de controle encerrada");
}

/// Encaminha um pedido ao ator e devolve a resposta. `false` se a conexão deve encerrar.
async fn responder(
    pedidos: &UnboundedSender<PedidoRecebido>,
    escrita: &mut (impl tokio::io::AsyncWrite + Unpin),
    pedido: Pedido,
) -> bool {
    let (responder, resposta) = oneshot::channel();
    if pedidos.send(PedidoRecebido { pedido, responder }).is_err() {
        return false; // o ator saiu; nada mais a fazer
    }
    let Ok(resposta) = resposta.await else {
        return false; // o ator largou a resposta
    };
    let msg = ParaInterface::Resposta(resposta);
    quadros::escrever(escrita, &msg).await.is_ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use ir_ipc::{Pedido, Resposta};
    use tokio::sync::mpsc;

    use super::super::escuta::{Acesso, Escuta};
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

    /// Um ator de mentira: responde todo pedido com `Feito`, para exercitar só o transporte.
    fn ator_de_mentira(mut pedidos: mpsc::UnboundedReceiver<PedidoRecebido>) {
        tokio::spawn(async move {
            while let Some(recebido) = pedidos.recv().await {
                let _ = recebido.responder.send(Resposta::Feito);
            }
        });
    }

    #[cfg(windows)]
    async fn conectar_cliente(
        endereco: &str,
    ) -> impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin {
        use tokio::net::windows::named_pipe::ClientOptions;
        // O servidor pode ainda não ter criado a instância; uma tentativa curta basta no teste.
        for _ in 0..50 {
            if let Ok(cliente) = ClientOptions::new().open(endereco) {
                return cliente;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("cliente não conectou");
    }

    #[cfg(not(windows))]
    async fn conectar_cliente(
        endereco: &str,
    ) -> impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin {
        for _ in 0..50 {
            if let Ok(cliente) = tokio::net::UnixStream::connect(endereco).await {
                return cliente;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("cliente não conectou");
    }

    #[tokio::test]
    async fn um_pedido_recebe_resposta_e_um_aviso_empurrado_chega() {
        let endereco = endereco_de_teste("controle");
        let escuta =
            Escuta::abrir(&endereco, Acesso::UsuarioInterativo).expect("abre o ponto de escuta");
        let (pedido_tx, pedido_rx) = mpsc::unbounded_channel();
        let (avisos, _) = broadcast::channel(16);
        ator_de_mentira(pedido_rx);
        tokio::spawn(servir(escuta, pedido_tx, avisos.clone()));

        let cliente = conectar_cliente(&endereco).await;
        let (mut leitura, mut escrita) = tokio::io::split(cliente);

        // Pede para acompanhar: a resposta volta, e a partir daí os avisos fluem.
        quadros::escrever(&mut escrita, &Pedido::Acompanhar)
            .await
            .expect("envia pedido");
        let resposta: ParaInterface = quadros::ler(&mut leitura)
            .await
            .expect("lê resposta")
            .expect("resposta presente");
        assert_eq!(resposta, ParaInterface::Resposta(Resposta::Feito));

        // Um aviso empurrado pelo serviço chega ao cliente que acompanha.
        avisos
            .send(Aviso::PareamentoConcluido { sucesso: true })
            .expect("há assinante");
        let empurrado: ParaInterface = quadros::ler(&mut leitura)
            .await
            .expect("lê aviso")
            .expect("aviso presente");
        assert_eq!(
            empurrado,
            ParaInterface::Aviso(Aviso::PareamentoConcluido { sucesso: true })
        );
    }
}
