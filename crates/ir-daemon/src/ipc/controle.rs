//! O canal de controle: serve [`Pedido`] e empurra [`Aviso`] para cada interface conectada.
//!
//! Uma tarefa por conexão. Ela lê pedidos, encaminha ao ator (o único dono do estado) por um
//! canal, espera a resposta e a devolve; e, em paralelo, repassa os avisos que o ator emite.
//! A tradução entre o estado interno e o [`ir_ipc::Estado`] publicado acontece no ator, não
//! aqui — este módulo só move bytes.

use std::time::Duration;

use ir_ipc::{Aviso, Falha, ParaInterface, Pedido, Resposta};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use super::escuta::{Chamada, Conexao, Escuta};
use super::{Ajudantes, PedidoRecebido, quadros};

/// Quanto tempo se espera o primeiro pedido de uma interface recusada, para responder a ele.
const ESPERA_DO_RECUSADO: Duration = Duration::from_secs(2);

/// Aceita conexões para sempre, uma tarefa por cliente.
pub(crate) async fn servir(
    mut escuta: Escuta,
    pedidos: UnboundedSender<PedidoRecebido>,
    avisos: broadcast::Sender<Aviso>,
    ajudantes: Ajudantes,
) {
    loop {
        match escuta.aceitar().await {
            Ok((conexao, Chamada::Permitida)) => {
                // Registrado em nível alto de propósito: é como se confirma, no diagnóstico, que
                // a janela achou o serviço em vez de ficar de fora em silêncio.
                info!("interface conectada");
                let leitor = escuta.leitor_de(&conexao);
                let pedidos = pedidos.clone();
                let avisos = avisos.subscribe();
                tokio::spawn(atender(conexao, leitor, pedidos, avisos, ajudantes.clone()));
            }
            Ok((conexao, Chamada::Negada { uid })) => {
                warn!(
                    uid,
                    "interface recusada: o usuário não pertence ao grupo `inputremote`; libere com \
                     `usermod -aG inputremote <usuário>`, que vale na conexão seguinte"
                );
                tokio::spawn(recusar(conexao));
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
    mut conexao: Conexao,
    leitor: ir_transferencia::Leitor,
    pedidos: UnboundedSender<PedidoRecebido>,
    mut avisos: broadcast::Receiver<Aviso>,
    ajudantes: Ajudantes,
) {
    // O primeiro pedido é lido antes de dividir a conexão: no Windows, só depois de ler algo do
    // pipe é que o sistema diz quem está do outro lado (`super::identidade`).
    let primeiro = match quadros::ler::<_, Pedido>(&mut conexao).await {
        Ok(Some(pedido)) => pedido,
        Ok(None) => return,
        Err(erro) => {
            warn!(%erro, "quadro de controle malformado; encerrando conexão");
            return;
        }
    };
    let leitor = super::escuta::leitor_depois_de_ler(&conexao, leitor);
    let (mut leitura, mut escrita) = tokio::io::split(conexao);
    // Só depois de a interface pedir para acompanhar é que os avisos começam a fluir; antes
    // disso ela recebe só as respostas aos próprios pedidos.
    let mut acompanhando = false;
    // Conta enquanto esta conexão viver, e só uma vez por conexão.
    let mut presenca = None;
    let mut proximo = Some(primeiro);
    // Por que esta conexão terminou. Sem isto, "ajudante de clipboard desligado" não distinguia o
    // cliente que fecha do aviso que não pôde ser escrito — e são defeitos diferentes.
    let mut motivo = "o laço terminou";
    loop {
        if let Some(pedido) = proximo.take() {
            acompanhando |= matches!(pedido, Pedido::Acompanhar | Pedido::AcompanharClipboard);
            if matches!(pedido, Pedido::AcompanharClipboard) && presenca.is_none() {
                info!("ajudante de clipboard ligado");
                presenca = Some(ajudantes.entrou());
            }
            if !responder(&pedidos, &mut escrita, pedido, leitor.clone()).await {
                motivo = "a resposta não pôde ser entregue";
                break;
            }
        }
        tokio::select! {
            quadro = quadros::ler::<_, Pedido>(&mut leitura) => match quadro {
                Ok(Some(pedido)) => proximo = Some(pedido),
                Ok(None) => {
                    motivo = "o cliente fechou";
                    break;
                }
                Err(erro) => {
                    warn!(%erro, "quadro de controle malformado; encerrando conexão");
                    motivo = "quadro malformado";
                    break;
                }
            },
            aviso = avisos.recv(), if acompanhando => {
                if !repassar(&mut escrita, aviso).await {
                    motivo = "o aviso não pôde ser escrito";
                    break;
                }
            }
        }
    }
    if presenca.is_some() {
        info!(motivo, "ajudante de clipboard desligado");
    }
    debug!("conexão de controle encerrada");
}

/// Manda um aviso à interface. `false` se a conexão deve encerrar.
async fn repassar(
    escrita: &mut (impl tokio::io::AsyncWrite + Unpin),
    aviso: Result<Aviso, broadcast::error::RecvError>,
) -> bool {
    match aviso {
        Ok(aviso) => quadros::escrever(escrita, &ParaInterface::Aviso(aviso))
            .await
            .is_ok(),
        // Uma interface lenta pode perder avisos; ela se reconcilia pelo próximo estado, então um
        // atraso não é motivo para derrubar a conexão.
        Err(broadcast::error::RecvError::Lagged(n)) => {
            debug!(perdidos = n, "interface atrasada perdeu avisos");
            true
        }
        Err(broadcast::error::RecvError::Closed) => false,
    }
}

/// Responde ao primeiro pedido de uma interface recusada com a razão da recusa, e fecha.
///
/// Fechar mudo deixaria a janela sem saber a diferença entre "o serviço caiu" e "você não tem
/// permissão" — e as duas pedem ações que não têm nada em comum. A recusa vai como resposta ao
/// primeiro pedido, e não por conta própria, para caber no pedido-resposta que a janela já espera.
async fn recusar(conexao: Conexao) {
    let (mut leitura, mut escrita) = tokio::io::split(conexao);
    let primeiro =
        tokio::time::timeout(ESPERA_DO_RECUSADO, quadros::ler::<_, Pedido>(&mut leitura)).await;
    if matches!(primeiro, Ok(Ok(Some(_)))) {
        let msg = ParaInterface::Resposta(Resposta::Falha(Falha::SemPermissao));
        let _ = quadros::escrever(&mut escrita, &msg).await;
    }
}

/// Encaminha um pedido ao ator e devolve a resposta. `false` se a conexão deve encerrar.
async fn responder(
    pedidos: &UnboundedSender<PedidoRecebido>,
    escrita: &mut (impl tokio::io::AsyncWrite + Unpin),
    pedido: Pedido,
    leitor: ir_transferencia::Leitor,
) -> bool {
    let (responder, resposta) = oneshot::channel();
    let recebido = PedidoRecebido {
        pedido,
        leitor,
        responder,
    };
    if pedidos.send(recebido).is_err() {
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
        tokio::spawn(servir(
            escuta,
            pedido_tx,
            avisos.clone(),
            Ajudantes::default(),
        ));

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

    /// O ajudante que se apresenta é contado enquanto a conexão dele vive — e só ele: a janela, que
    /// também acompanha, não conta. É a contagem que faz o serviço relançar o ajudante que falta.
    #[tokio::test]
    async fn o_ajudante_conta_enquanto_esta_ligado_e_a_janela_nao() {
        let endereco = endereco_de_teste("ajudantes");
        let escuta =
            Escuta::abrir(&endereco, Acesso::UsuarioInterativo).expect("abre o ponto de escuta");
        let (pedido_tx, pedido_rx) = mpsc::unbounded_channel();
        let (avisos, _) = broadcast::channel(16);
        ator_de_mentira(pedido_rx);
        let ajudantes = Ajudantes::default();
        tokio::spawn(servir(escuta, pedido_tx, avisos, ajudantes.clone()));

        let mut janela = conectar_cliente(&endereco).await;
        quadros::escrever(&mut janela, &Pedido::Acompanhar)
            .await
            .unwrap();
        let _: Option<ParaInterface> = quadros::ler(&mut janela).await.unwrap();
        assert_eq!(ajudantes.ligados(), 0, "a janela não é ajudante");

        let mut ajudante = conectar_cliente(&endereco).await;
        for _ in 0..2 {
            quadros::escrever(&mut ajudante, &Pedido::AcompanharClipboard)
                .await
                .unwrap();
            let _: Option<ParaInterface> = quadros::ler(&mut ajudante).await.unwrap();
        }
        assert_eq!(ajudantes.ligados(), 1, "uma conexão conta uma vez");

        drop(ajudante);
        for _ in 0..100 {
            if ajudantes.ligados() == 0 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("a conexão fechou e o ajudante continuou contado");
    }
}
