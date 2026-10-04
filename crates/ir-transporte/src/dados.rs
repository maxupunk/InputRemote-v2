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
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use ir_crypto::{Identity, PublicKey};
use ir_net::bulk::{self, BulkReceiver, BulkSender, Frames};
use ir_proto::carrier::Carrier;
use ir_proto::codec;
use ir_proto::frame::{Frame, Sequence};
use ir_proto::message::{BulkMessage, Message};
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};

/// Por que uma mensagem não saiu — e se o enlace sobreviveu a isso.
///
/// As duas causas eram um erro só, e quem enviava tratava qualquer uma como queda. Foi assim que
/// um manifesto grande demais para o quadro, defeito **daqui**, chegou à tela como "a conexão de
/// arquivos caiu" ([log 55](../../../docs/logs/55-o-manifesto-que-nao-cabia.md)): o diagnóstico
/// apontava para a rede, e o enlace saudável era derrubado à toa.
#[derive(Debug)]
pub enum FalhaDeEnvio {
    /// A mensagem não vira quadro: passa do teto do portador. Nada foi escrito no socket, então o
    /// enlace segue de pé — o defeito é de quem montou a mensagem.
    NaoCabe(ir_proto::ProtoError),
    /// O Noise ou o socket falharam: o enlace caiu.
    Enlace(anyhow::Error),
}

impl std::fmt::Display for FalhaDeEnvio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NaoCabe(erro) => write!(f, "a mensagem não cabe num quadro de dados: {erro}"),
            Self::Enlace(erro) => write!(f, "o enlace de dados falhou: {erro:#}"),
        }
    }
}

impl std::error::Error for FalhaDeEnvio {}

impl FalhaDeEnvio {
    /// Se o enlace caiu com esta falha. Quando não caiu, ele continua servindo as próximas cópias.
    #[must_use]
    pub const fn derrubou_o_enlace(&self) -> bool {
        matches!(self, Self::Enlace(_))
    }
}

/// Quantos bytes o canal de dados moveu, nos dois sentidos, desde que foi criado.
///
/// Uma contagem só para todos os enlaces que a porta abrir: a tela quer saber se **algo** está
/// atravessando — uma cópia, a pasta —, e não de qual enlace. Contado no quadro cifrado, que é o
/// que a rede carrega.
#[derive(Debug, Default)]
pub struct Contador {
    enviados: AtomicU64,
    recebidos: AtomicU64,
}

impl Contador {
    /// Os bytes que saíram.
    #[must_use]
    pub fn enviados(&self) -> u64 {
        self.enviados.load(Ordering::Relaxed)
    }

    /// Os bytes que chegaram.
    #[must_use]
    pub fn recebidos(&self) -> u64 {
        self.recebidos.load(Ordering::Relaxed)
    }

    fn somar(contagem: &AtomicU64, bytes: usize) {
        contagem.fetch_add(u64::try_from(bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

/// A metade que manda mensagens do canal 5.
#[derive(Debug)]
pub struct Remetente {
    saida: BulkSender<WriteHalf<TcpStream>>,
    proxima: u32,
    contador: Arc<Contador>,
}

impl Remetente {
    /// Manda uma mensagem, deixando o TCP juntar segmentos.
    ///
    /// É a forma dos blocos de arquivo: forçar a saída a cada bloco impediria o TCP de formar
    /// segmentos cheios, que é exatamente o que se quer que ele faça.
    ///
    /// # Errors
    ///
    /// [`FalhaDeEnvio::NaoCabe`] se a mensagem não couber no quadro; [`FalhaDeEnvio::Enlace`] se o
    /// Noise recusar ou o socket falhar.
    pub async fn enviar(&mut self, mensagem: BulkMessage) -> Result<(), FalhaDeEnvio> {
        let bytes = self.codificar(mensagem)?;
        self.saida
            .send(&bytes)
            .await
            .context("enviando o quadro")
            .map_err(FalhaDeEnvio::Enlace)?;
        Contador::somar(&self.contador.enviados, bytes.len());
        Ok(())
    }

    /// Manda uma mensagem e força a saída, para quando o par está esperando por ela.
    ///
    /// É a forma do manifesto, do `Accept`, do `Verified` e do `Cancel`: mensagens pequenas cuja
    /// demora o usuário sente como "a transferência não começa".
    ///
    /// # Errors
    ///
    /// Os mesmos de [`Self::enviar`].
    pub async fn enviar_agora(&mut self, mensagem: BulkMessage) -> Result<(), FalhaDeEnvio> {
        let bytes = self.codificar(mensagem)?;
        self.saida
            .send_now(&bytes)
            .await
            .context("enviando o quadro")
            .map_err(FalhaDeEnvio::Enlace)?;
        Contador::somar(&self.contador.enviados, bytes.len());
        Ok(())
    }

    /// Embrulha a mensagem num quadro do protocolo e o codifica para o portador TCP.
    ///
    /// O número de sequência é contado aqui e serve só para diagnóstico: sobre TCP a ordem é do
    /// meio, e `ChannelId::Bulk` não entra na confiabilidade de aplicação
    /// ([03, §4](../../../docs/03-protocolo.md)).
    fn codificar(&mut self, mensagem: BulkMessage) -> Result<Vec<u8>, FalhaDeEnvio> {
        let sequencia = Sequence(self.proxima);
        self.proxima = self.proxima.wrapping_add(1);
        let quadro = Frame::new(Message::Bulk(mensagem), sequencia);
        codec::encode(&quadro, Carrier::Tcp).map_err(FalhaDeEnvio::NaoCabe)
    }
}

/// A metade que recebe mensagens do canal 5.
#[derive(Debug)]
pub struct Destinatario {
    entrada: BulkReceiver<ReadHalf<TcpStream>>,
    contador: Arc<Contador>,
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
        Contador::somar(&self.contador.recebidos, bytes.len());
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
    contador: Arc<Contador>,
}

impl Porta {
    /// Abre a escuta.
    ///
    /// # Errors
    ///
    /// Se a porta não puder ser vinculada — quase sempre porque outra instância já a ocupa.
    /// `contador` soma o que todo enlace aberto por esta porta move; quem o criou o lê.
    pub async fn abrir(
        porta: u16,
        identidade: Arc<Identity>,
        contador: Arc<Contador>,
    ) -> Result<Self> {
        let endereco = SocketAddr::from(([0, 0, 0, 0], porta));
        let escuta = bulk::bind(endereco)
            .await
            .with_context(|| format!("vinculando o TCP de dados em {endereco}"))?;
        Ok(Self {
            escuta,
            identidade,
            contador,
        })
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
        Ok(partir(enlace, esperado, &self.contador))
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
        Ok(partir(enlace, esperado, &self.contador))
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
fn partir(
    enlace: bulk::BulkLink<TcpStream>,
    par: PublicKey,
    contador: &Arc<Contador>,
) -> EnlaceDeDados {
    let (entrada, saida) = enlace.split();
    EnlaceDeDados {
        remetente: Remetente {
            saida,
            proxima: 0,
            contador: Arc::clone(contador),
        },
        destinatario: Destinatario {
            entrada,
            contador: Arc::clone(contador),
        },
        par,
    }
}

#[cfg(test)]
mod testes;
