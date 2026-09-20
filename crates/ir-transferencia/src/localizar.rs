//! Onde está o par na rede, quando a configuração não diz.
//!
//! Parear pelo Bluetooth grava só o endereço do rádio, e arquivo nunca viaja pelo rádio. O canal de
//! arquivos ficava então esperando para sempre um endereço que ninguém daria: teclado, mouse e texto
//! atravessavam, e copiar um arquivo não fazia nada. Quem pergunta à rede local "onde está esta
//! máquina?" é a descoberta; aqui só se decide **quando** perguntar.

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use ir_crypto::PublicKey;
use tracing::info;

/// A pergunta "onde está, na rede, o par com esta chave?". `None` quando ninguém respondeu.
///
/// Uma função, e não um tipo do `ir-transporte`: o serviço liga a descoberta de verdade, e o teste
/// liga uma resposta pronta — sem broadcast, sem depender da rede de quem roda os testes.
pub type Localizador = Arc<
    dyn Fn(PublicKey) -> Pin<Box<dyn Future<Output = Option<SocketAddr>> + Send>> + Send + Sync,
>;

/// Um localizador que nunca acha: o canal só disca o endereço configurado.
#[must_use]
pub fn sem_localizador() -> Localizador {
    Arc::new(|_| Box::pin(async { None }))
}

/// Para onde discar: o endereço de rede configurado, e senão onde a rede diz que o par está.
///
/// `falhou` é o endereço que acabou de não atender: um endereço configurado que envelheceu (o DHCP
/// deu outro) também é resolvido pela rede, em vez de ser tentado para sempre.
pub(crate) async fn onde_discar(
    localizar: &Localizador,
    configurado: Option<SocketAddr>,
    falhou: Option<SocketAddr>,
    par: PublicKey,
) -> Option<SocketAddr> {
    if configurado.is_some() && configurado != falhou {
        return configurado;
    }
    let achado = localizar(par).await?;
    if Some(achado) == falhou {
        return None;
    }
    info!(%achado, "par achado na rede local para o canal de arquivos");
    Some(achado)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixo(endereco: Option<SocketAddr>) -> Localizador {
        Arc::new(move |_| Box::pin(async move { endereco }))
    }

    fn par() -> PublicKey {
        ir_crypto::Identity::generate().public()
    }

    fn end(texto: &str) -> Option<SocketAddr> {
        texto.parse().ok()
    }

    #[tokio::test]
    async fn o_configurado_vem_primeiro() {
        let rede = fixo(end("10.0.0.9:1"));
        let achado = onde_discar(&rede, end("10.0.0.5:1"), None, par()).await;
        assert_eq!(achado, end("10.0.0.5:1"));
    }

    #[tokio::test]
    async fn sem_configurado_pergunta_a_rede() {
        let achado = onde_discar(&fixo(end("10.0.0.9:1")), None, None, par()).await;
        assert_eq!(achado, end("10.0.0.9:1"));
    }

    #[tokio::test]
    async fn o_configurado_que_nao_atendeu_da_lugar_ao_da_rede() {
        let velho = end("10.0.0.5:1");
        let achado = onde_discar(&fixo(end("10.0.0.9:1")), velho, velho, par()).await;
        assert_eq!(achado, end("10.0.0.9:1"));
    }

    #[tokio::test]
    async fn a_rede_repetindo_o_que_falhou_nao_e_resposta() {
        let velho = end("10.0.0.5:1");
        assert_eq!(onde_discar(&fixo(velho), None, velho, par()).await, None);
        assert_eq!(
            onde_discar(&sem_localizador(), None, None, par()).await,
            None
        );
    }
}
