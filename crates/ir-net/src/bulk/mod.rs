//! O canal de dados: clipboard grande, imagens e arquivos, sobre TCP.
//!
//! É o portador do canal 5 do protocolo, e a única coisa que ele tem em comum com o UDP deste
//! mesmo crate é o `ir-crypto`. Tudo o mais difere, porque o requisito difere: aqui integridade
//! vale mais que latência, e é o oposto exato dos canais de entrada
//! ([ADR-0010](../../../docs/adr/0010-canal-de-dados-em-tcp-proprio.md)).
//!
//! Três diferenças que valem ser ditas de frente:
//!
//! 1. **Não há pareamento.** A identidade já foi fixada pela sessão de entrada, então o padrão
//!    Noise é sempre `IK` e não existe código de seis dígitos neste caminho.
//! 2. **Não há canal de comandos nem de eventos.** Quem transfere segura o enlace na mão. Uma
//!    fila entre o motor e o socket precisaria de limite, e um limite errado é ou um estouro de
//!    memória com 5 GB em voo, ou um impasse entre duas filas cheias. O `await` do socket **já
//!    é** a contrapressão certa, de graça.
//! 3. **Um quadro que não abre derruba o enlace**, como no RFCOMM e ao contrário do UDP: sobre
//!    stream, a tag falhar significa que as contagens das duas pontas divergiram.

pub mod handshake;
pub mod link;
pub mod stream;
pub mod wire;

use std::net::SocketAddr;

use ir_crypto::PublicKey;
use tokio::net::{TcpListener, TcpStream};

pub use handshake::{accept, dial};
pub use link::{BulkLink, BulkReceiver, BulkSender};
pub use stream::{Channel, FrameReader, FrameWriter, Frames};
pub use wire::{Framer, MAX_BODY};

use crate::error::Result;

/// Abre a escuta do canal de dados.
///
/// # Errors
///
/// [`NetError::Io`](crate::NetError::Io) se a porta não puder ser vinculada — em geral porque
/// outra instância já está ouvindo nela.
pub async fn bind(addr: SocketAddr) -> Result<TcpListener> {
    Ok(TcpListener::bind(addr).await?)
}

/// Disca para o par.
///
/// # Errors
///
/// [`NetError::Io`](crate::NetError::Io) se não houver ninguém atendendo, o que é o caso normal
/// enquanto a outra máquina não subiu.
pub async fn connect(addr: SocketAddr) -> Result<TcpStream> {
    let stream = TcpStream::connect(addr).await?;
    prepare(&stream);
    Ok(stream)
}

/// Ajusta um socket recém-aceito ou recém-conectado.
///
/// `TCP_NODELAY` ligado. Nos blocos de arquivo ele não muda nada — segmentos de 60 KiB já saem
/// cheios —, mas as respostas do canal são pequenas (`Accept`, `Verified`, `Cancel`) e sem ele o
/// algoritmo de Nagle as retém esperando mais bytes que não vêm. A consequência seria um
/// `Accept` de 12 bytes chegando dezenas de milissegundos depois de pronto, e a transferência
/// parecendo lenta para começar.
pub fn prepare(stream: &TcpStream) {
    // Falhar aqui não impede nada de funcionar: é ajuste de desempenho, não de correção.
    let _ = stream.set_nodelay(true);
}

/// Qual dos dois enlaces sobrevive quando as duas máquinas discam ao mesmo tempo.
///
/// Devolve `true` se este lado deve ficar com o enlace **que ele mesmo abriu**, e `false` se deve
/// ficar com o que recebeu.
///
/// # Por que isto é necessário
///
/// As duas máquinas escutam e as duas podem ter o endereço da outra — é exatamente a
/// configuração da bancada, onde cada lado tem o `peer_addr` do outro. Sem regra, um religar de
/// rede produz duas conexões simultâneas e a transferência fica dependendo de qual delas o motor
/// pegou primeiro: às vezes funciona.
///
/// # A regra
///
/// **Sobrevive o enlace cujo iniciador tem a chave pública maior**, em ordem de bytes. Cada lado
/// aplica isso do seu ponto de vista e os dois chegam à mesma conclusão, sem trocar mensagem
/// nenhuma: as duas chaves já são conhecidas dos dois lados desde o pareamento.
///
/// Ela só é consultada **na colisão**. Com uma conexão só, ela fica de pé — senão o lado de chave
/// menor derrubaria o único enlace que existe quando ele é o único que sabe o endereço do outro.
///
/// # Chaves iguais
///
/// Só acontece se a máquina estiver falando com a própria identidade, o que não é uma topologia
/// do produto. Não existe escolha consistente nesse caso — qualquer regra simétrica faz os dois
/// lados descartarem o que o outro guardou —, então a resposta é `false` e o enlace não se
/// estabelece, que é a falha segura.
#[must_use]
pub fn keep_outbound_on_collision(local: PublicKey, peer: PublicKey) -> bool {
    local.0 > peer.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(first: u8) -> PublicKey {
        let mut bytes = [0u8; 32];
        if let Some(slot) = bytes.first_mut() {
            *slot = first;
        }
        PublicKey(bytes)
    }

    #[test]
    fn exactly_one_side_keeps_its_own_connection() {
        // A propriedade que faz a regra funcionar sem troca de mensagem: aplicada dos dois lados,
        // ela nomeia **um** sobrevivente. Se os dois guardassem o próprio, haveria dois enlaces;
        // se nenhum, zero.
        let a = key(9);
        let b = key(2);
        assert!(keep_outbound_on_collision(a, b));
        assert!(!keep_outbound_on_collision(b, a));
    }

    #[test]
    fn the_rule_is_the_same_every_time_it_is_asked() {
        let a = key(3);
        let b = key(7);
        for _ in 0..8 {
            assert!(!keep_outbound_on_collision(a, b));
            assert!(keep_outbound_on_collision(b, a));
        }
    }

    #[test]
    fn the_comparison_looks_past_the_first_byte() {
        // Chaves reais diferem em qualquer posição. Uma regra que só olhasse o primeiro byte
        // empataria em 1 de 256 pares.
        let mut menor = [0u8; 32];
        let mut maior = [0u8; 32];
        if let Some(slot) = menor.get_mut(31) {
            *slot = 1;
        }
        if let Some(slot) = maior.get_mut(31) {
            *slot = 2;
        }
        assert!(keep_outbound_on_collision(
            PublicKey(maior),
            PublicKey(menor)
        ));
        assert!(!keep_outbound_on_collision(
            PublicKey(menor),
            PublicKey(maior)
        ));
    }

    #[test]
    fn talking_to_its_own_identity_fails_safe() {
        let same = key(5);
        assert!(!keep_outbound_on_collision(same, same));
    }
}
