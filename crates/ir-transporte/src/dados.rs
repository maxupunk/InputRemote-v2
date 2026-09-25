//! A segunda porta: o canal de dados, para clipboard grande, imagens e arquivos.
//!
//! Ao lado do [`Transporte`](crate::Transporte), e não dentro dele. As razões estão no
//! [ADR-0010](../../../docs/adr/0010-canal-de-dados-em-tcp-proprio.md), e a curta é que os dois
//! têm requisitos opostos: o de entrada quer latência e mensagens minúsculas no compasso de 5 ms
//! da sessão; este quer integridade e blocos de 60 KiB, e não pode encostar naquele caminho.
//!
//! # O que esta porta entrega, e por quê
//!
//! O transporte de entrada entrega um *handle* que aceita comandos: o serviço diz "envie" e
//! esquece. Aqui não — quem transfere recebe **as duas metades do enlace na mão** e as segura pela
//! transferência inteira.
//!
//! A diferença não é de gosto. Uma fila entre o motor e o socket precisaria de um limite, e um
//! limite errado é ou gigabytes em voo na memória, ou um impasse entre duas filas cheias. O
//! `await` do socket já é a contrapressão certa, e de graça — mas só chega a quem segura o socket.
//!
//! # Duas metades, e não uma
//!
//! Enquanto um lado despeja blocos, o outro devolve confirmações. Fazer as duas coisas com um
//! objeto só exigiria `select!`, e cancelar uma leitura no meio perderia bytes que o TCP já
//! considera entregues. Ver `ir_net::bulk::stream`.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use ir_crypto::{Identity, PublicKey};
use ir_net::bulk::{self, BulkReceiver, BulkSender, Frames};
use ir_proto::carrier::Carrier;
use ir_proto::codec;
use ir_proto::frame::{Frame, Sequence};
use ir_proto::message::{BulkMessage, Message};
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};

/// A metade que manda mensagens do canal 5.
#[derive(Debug)]
pub struct Remetente {
    saida: BulkSender<WriteHalf<TcpStream>>,
    proxima: u32,
}

impl Remetente {
    /// Manda uma mensagem, deixando o TCP juntar segmentos.
    ///
    /// É a forma dos blocos de arquivo: forçar a saída a cada bloco impediria o TCP de formar
    /// segmentos cheios, que é exatamente o que se quer que ele faça.
    ///
    /// # Errors
    ///
    /// Se a mensagem não couber no quadro, se o Noise recusar, ou se o socket falhar.
    pub async fn enviar(&mut self, mensagem: BulkMessage) -> Result<()> {
        let bytes = self.codificar(mensagem)?;
        self.saida.send(&bytes).await.context("enviando o quadro")
    }

    /// Manda uma mensagem e força a saída, para quando o par está esperando por ela.
    ///
    /// É a forma do manifesto, do `Accept`, do `Verified` e do `Cancel`: mensagens pequenas cuja
    /// demora o usuário sente como "a transferência não começa".
    ///
    /// # Errors
    ///
    /// Os mesmos de [`Self::enviar`].
    pub async fn enviar_agora(&mut self, mensagem: BulkMessage) -> Result<()> {
        let bytes = self.codificar(mensagem)?;
        self.saida
            .send_now(&bytes)
            .await
            .context("enviando o quadro")
    }

    /// Embrulha a mensagem num quadro do protocolo e o codifica para o portador TCP.
    ///
    /// O número de sequência é contado aqui e serve só para diagnóstico: sobre TCP a ordem é do
    /// meio, e `ChannelId::Bulk` não entra na confiabilidade de aplicação
    /// ([03, §4](../../../docs/03-protocolo.md)).
    fn codificar(&mut self, mensagem: BulkMessage) -> Result<Vec<u8>> {
        let sequencia = Sequence(self.proxima);
        self.proxima = self.proxima.wrapping_add(1);
        let quadro = Frame::new(Message::Bulk(mensagem), sequencia);
        codec::encode(&quadro, Carrier::Tcp).context("codificando o quadro de dados")
    }
}

/// A metade que recebe mensagens do canal 5.
#[derive(Debug)]
pub struct Destinatario {
    entrada: BulkReceiver<ReadHalf<TcpStream>>,
}

impl Destinatario {
    /// Espera a próxima mensagem do par.
    ///
    /// # Errors
    ///
    /// Se o par encerrar, se um quadro não abrir — e aí o enlace **precisa** cair, porque as
    /// contagens divergiram —, ou se chegar um quadro que não é do canal de dados.
    pub async fn receber(&mut self) -> Result<BulkMessage> {
        let bytes = self.entrada.recv().await.context("lendo o quadro")?;
        let quadro = codec::decode(&bytes, Carrier::Tcp).context("decodificando o quadro")?;
        match quadro.message {
            Message::Bulk(mensagem) => Ok(mensagem),
            // O codec já recusa canal que não pode viajar por TCP, mas controle e clipboard de
            // texto podem — e não é aqui que eles chegam. Este enlace carrega o canal 5 e nada
            // mais; qualquer outra coisa é o par falando outro protocolo.
            outra => bail!("quadro fora do canal de dados: {:?}", outra.channel()),
        }
    }
}

/// Um enlace de dados aberto com o par.
#[derive(Debug)]
pub struct EnlaceDeDados {
    /// Para mandar.
    pub remetente: Remetente,
    /// Para receber.
    pub destinatario: Destinatario,
    /// A identidade que o par apresentou — a mesma que a sessão fixou.
    pub par: PublicKey,
}

/// Quanto o handshake do canal de dados pode levar.
///
/// Sem prazo, quem conectasse e ficasse calado prendia a porta: o laço que atende esperava o
/// handshake dele, e o par de verdade não era atendido.
const PRAZO_DO_HANDSHAKE: std::time::Duration = std::time::Duration::from_secs(10);

/// A porta TCP do canal de dados: escuta, e disca quando pedido.
#[derive(Debug)]
pub struct Porta {
    escuta: TcpListener,
    identidade: Arc<Identity>,
}

impl Porta {
    /// Abre a escuta.
    ///
    /// # Errors
    ///
    /// Se a porta não puder ser vinculada — quase sempre porque outra instância já a ocupa.
    pub async fn abrir(porta: u16, identidade: Arc<Identity>) -> Result<Self> {
        let endereco = SocketAddr::from(([0, 0, 0, 0], porta));
        let escuta = bulk::bind(endereco)
            .await
            .with_context(|| format!("vinculando o TCP de dados em {endereco}"))?;
        Ok(Self { escuta, identidade })
    }

    /// Em que endereço esta porta está escutando.
    ///
    /// Serve ao teste, que precisa de porta efêmera, e ao diagnóstico.
    ///
    /// # Errors
    ///
    /// Se o socket não souber o próprio endereço.
    pub fn endereco(&self) -> Result<SocketAddr> {
        self.escuta
            .local_addr()
            .context("lendo o endereço da escuta")
    }

    /// Atende a próxima conexão e confere a identidade de quem discou.
    ///
    /// # Errors
    ///
    /// Se o `accept` falhar, se o handshake não fechar, ou se quem discou apresentar outra
    /// identidade — e recusar é o comportamento certo.
    pub async fn aceitar(&self, esperado: PublicKey) -> Result<EnlaceDeDados> {
        let (socket, _) = self.escuta.accept().await.context("aceitando conexão")?;
        bulk::prepare(&socket);
        let enlace = tokio::time::timeout(
            PRAZO_DO_HANDSHAKE,
            bulk::accept(Frames::new(socket), &self.identidade, esperado),
        )
        .await
        .context("o handshake do canal de dados não terminou no prazo")?
        .context("handshake do canal de dados")?;
        Ok(partir(enlace, esperado))
    }

    /// Disca para o par.
    ///
    /// # Errors
    ///
    /// Se não houver ninguém atendendo — o caso normal enquanto a outra máquina não subiu —, ou se
    /// o handshake não fechar.
    pub async fn discar(&self, alvo: SocketAddr, esperado: PublicKey) -> Result<EnlaceDeDados> {
        let socket = bulk::connect(alvo)
            .await
            .with_context(|| format!("conectando o TCP de dados em {alvo}"))?;
        let enlace = tokio::time::timeout(
            PRAZO_DO_HANDSHAKE,
            bulk::dial(Frames::new(socket), &self.identidade, esperado),
        )
        .await
        .context("o handshake do canal de dados não terminou no prazo")?
        .context("handshake do canal de dados")?;
        Ok(partir(enlace, esperado))
    }
}

/// Se este lado fica com o enlace que ele mesmo abriu, quando os dois discaram ao mesmo tempo.
///
/// Repassa a regra do `ir-net`, para o serviço não precisar conhecê-lo direto
/// ([02, §2](../../../docs/02-arquitetura.md)).
#[must_use]
pub fn ficar_com_o_proprio(local: PublicKey, par: PublicKey) -> bool {
    bulk::keep_outbound_on_collision(local, par)
}

/// Parte um enlace nas duas metades que o serviço usa.
fn partir(enlace: bulk::BulkLink<TcpStream>, par: PublicKey) -> EnlaceDeDados {
    let (entrada, saida) = enlace.split();
    EnlaceDeDados {
        remetente: Remetente { saida, proxima: 0 },
        destinatario: Destinatario { entrada },
        par,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use ir_proto::message::{CancelReason, ManifestItem, TransferId};

    use super::*;

    /// Duas portas em loopback, já com as identidades fixadas uma na outra.
    async fn ligadas() -> (EnlaceDeDados, EnlaceDeDados) {
        let aqui = Arc::new(Identity::generate());
        let la = Arc::new(Identity::generate());
        let (chave_daqui, chave_de_la) = (aqui.public(), la.public());

        // Porta efêmera: o teste não pode brigar com a 52525 de um serviço instalado.
        let porta_de_la = Porta::abrir(0, Arc::clone(&la)).await.expect("escuta");
        let alvo = porta_de_la.endereco().expect("endereço");
        let alvo = SocketAddr::from(([127, 0, 0, 1], alvo.port()));

        let atende = tokio::spawn(async move { porta_de_la.aceitar(chave_daqui).await });
        let porta_daqui = Porta::abrir(0, aqui).await.expect("escuta");
        let discado = porta_daqui.discar(alvo, chave_de_la).await.expect("disca");
        let atendido = atende.await.expect("tarefa").expect("atende");
        (discado, atendido)
    }

    #[tokio::test]
    async fn uma_mensagem_atravessa_o_socket_de_verdade() {
        let (mut daqui, mut de_la) = ligadas().await;
        let manifesto = BulkMessage::Manifest {
            id: TransferId(7),
            items: vec![ManifestItem {
                path: "relat\u{f3}rio/a.pdf".to_owned(),
                size: 42,
                is_dir: false,
            }],
            total_bytes: 42,
        };
        daqui
            .remetente
            .enviar_agora(manifesto.clone())
            .await
            .unwrap();
        assert_eq!(de_la.destinatario.receber().await.unwrap(), manifesto);
    }

    #[tokio::test]
    async fn as_duas_direcoes_funcionam_ao_mesmo_tempo() {
        // O padrão real: um lado despeja blocos enquanto o outro confirma.
        let (mut daqui, mut de_la) = ligadas().await;
        let id = TransferId(1);

        let despeja = tokio::spawn(async move {
            for n in 0..16u32 {
                daqui
                    .remetente
                    .enviar(BulkMessage::FileBlock {
                        id,
                        item: 0,
                        offset: u64::from(n) * 1024,
                        data: vec![u8::try_from(n).unwrap_or(0); 1024],
                    })
                    .await
                    .unwrap();
            }
            // Lê as confirmações que voltaram.
            let mut confirmadas = 0;
            while confirmadas < 16 {
                let voltou = daqui.destinatario.receber().await.unwrap();
                confirmadas += usize::from(matches!(voltou, BulkMessage::Verified { .. }));
            }
            confirmadas
        });

        for n in 0..16u32 {
            match de_la.destinatario.receber().await.unwrap() {
                BulkMessage::FileBlock { offset, data, .. } => {
                    assert_eq!(offset, u64::from(n) * 1024);
                    assert_eq!(data.len(), 1024);
                }
                outra => panic!("esperava um bloco, veio {outra:?}"),
            }
            de_la
                .remetente
                .enviar_agora(BulkMessage::Verified {
                    id,
                    item: 0,
                    ok: true,
                })
                .await
                .unwrap();
        }
        assert_eq!(despeja.await.unwrap(), 16);
    }

    #[tokio::test]
    async fn quem_atende_recusa_uma_identidade_que_nao_e_a_fixada() {
        // A porta do canal de dados é alcançável por qualquer um na rede local. O que impede um
        // estranho de abrir uma transferência é esta conferência, e nada mais.
        let la = Arc::new(Identity::generate());
        let estranho = Arc::new(Identity::generate());
        let fixada = Identity::generate().public();
        let chave_de_la = la.public();

        let porta_de_la = Porta::abrir(0, Arc::clone(&la)).await.expect("escuta");
        let alvo = porta_de_la.endereco().expect("endereço");
        let alvo = SocketAddr::from(([127, 0, 0, 1], alvo.port()));

        let atende = tokio::spawn(async move { porta_de_la.aceitar(fixada).await });
        let porta_do_estranho = Porta::abrir(0, estranho).await.expect("escuta");
        let _ = porta_do_estranho.discar(alvo, chave_de_la).await;
        assert!(
            atende.await.expect("tarefa").is_err(),
            "um estranho não pode abrir o canal de dados"
        );
    }

    #[tokio::test]
    async fn um_quadro_fora_do_canal_de_dados_e_recusado() {
        // O enlace carrega o canal 5 e nada mais. Aceitar controle por aqui seria um segundo
        // caminho para a máquina de estados da sessão, que é exatamente o que o ADR-0010 separou.
        let (mut daqui, mut de_la) = ligadas().await;
        let quadro = Frame::new(
            Message::Control(ir_proto::message::Control::AckOnly),
            Sequence(0),
        );
        // Codificado como UDP, porque o codec recusaria controle... não: controle viaja em
        // qualquer portador. É o `receber` que tem de barrar.
        let bytes = codec::encode(&quadro, Carrier::Tcp).expect("controle cabe no TCP");
        daqui.remetente.saida.send_now(&bytes).await.unwrap();
        assert!(de_la.destinatario.receber().await.is_err());
    }

    #[tokio::test]
    async fn o_cancelamento_atravessa_como_qualquer_outra_mensagem() {
        let (mut daqui, mut de_la) = ligadas().await;
        let cancel = BulkMessage::Cancel {
            id: TransferId(3),
            reason: CancelReason::UserRequested,
        };
        daqui.remetente.enviar_agora(cancel.clone()).await.unwrap();
        assert_eq!(de_la.destinatario.receber().await.unwrap(), cancel);
    }

    #[test]
    fn a_regra_da_colisao_chega_ao_servico_sem_ele_conhecer_o_ir_net() {
        let maior = PublicKey([9; 32]);
        let menor = PublicKey([1; 32]);
        assert!(ficar_com_o_proprio(maior, menor));
        assert!(!ficar_com_o_proprio(menor, maior));
    }
}
