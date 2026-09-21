//! Como este lado consegue um enlace de dados: atendendo, discando, ou os dois.
//!
//! Separado do laço de vida do canal porque é outra pergunta. Ali se decide *quando* há canal;
//! aqui, *com quem falar* — o endereço da configuração, o que a descoberta achar, e a regra de
//! colisão que evita os dois lados discando ao mesmo tempo.

use std::net::SocketAddr;
use std::time::Duration;

use ir_crypto::PublicKey;
use ir_transporte::dados::{EnlaceDeDados, Porta, ficar_com_o_proprio};
use tracing::debug;

use crate::localizar::Localizador;
use crate::{
    Ajuste, CARENCIA_DO_NAO_PREFERIDO, ESPERA_ENTRE_TENTATIVAS, ESPERA_SEM_ENDERECO, localizar,
};

/// Consegue um enlace: atende quem chega, e disca quando é a vez deste lado.
///
/// Os dois lados escutam e os dois podem ter o endereço do outro. Quem disca primeiro é decidido
/// pela regra da chave maior, sem trocar mensagem; o outro só disca depois da carência, para o caso
/// de ser ele o único que sabe o endereço.
pub(crate) async fn obter(
    porta: &Porta,
    ajuste: &Ajuste,
    par: PublicKey,
    alvo: Option<ir_transporte::Endereco>,
) -> EnlaceDeDados {
    let nossa = ajuste.identidade.public();
    let carencia = if ficar_com_o_proprio(nossa, par) {
        Duration::ZERO
    } else {
        CARENCIA_DO_NAO_PREFERIDO
    };

    let configurado = alvo_de_rede(alvo);
    let mut falhou = None;
    loop {
        tokio::select! {
            atendido = porta.aceitar(par) => match atendido {
                Ok(enlace) => return enlace,
                Err(erro) => debug!(%erro, "conexão de arquivos recusada na porta"),
            },
            discado = discar_depois(porta, &ajuste.localizar, (configurado, falhou), par, carencia) => {
                match discado {
                    Ok(enlace) => return enlace,
                    Err(nao_atendeu) => falhou = nao_atendeu,
                }
            }
        }
    }
}

/// Disca depois da carência. O erro diz qual endereço não atendeu (nenhum, se não havia onde
/// discar), para o laço tentar de novo — e perguntar à rede em vez de insistir nele.
///
/// `(configurado, falhou)`: o endereço de rede da configuração, e o último que não atendeu.
async fn discar_depois(
    porta: &Porta,
    localizar: &Localizador,
    (configurado, falhou): (Option<SocketAddr>, Option<SocketAddr>),
    par: PublicKey,
    carencia: Duration,
) -> Result<EnlaceDeDados, Option<SocketAddr>> {
    tokio::time::sleep(carencia).await;
    let Some(alvo) = localizar::onde_discar(localizar, configurado, falhou, par).await else {
        // Ninguém na rede disse onde o par está: ele está desligado, ou longe. Perguntar de novo
        // logo só encheria a rede de broadcast.
        tokio::time::sleep(ESPERA_SEM_ENDERECO).await;
        return Err(None);
    };
    match porta.discar(alvo, par).await {
        Ok(enlace) => Ok(enlace),
        Err(erro) => {
            debug!(%erro, %alvo, "o par ainda não atende no canal de arquivos");
            tokio::time::sleep(ESPERA_ENTRE_TENTATIVAS).await;
            Err(Some(alvo))
        }
    }
}

/// O endereço de rede do par, quando o que se sabe dele é de rede.
const fn alvo_de_rede(alvo: Option<ir_transporte::Endereco>) -> Option<SocketAddr> {
    match alvo {
        Some(ir_transporte::Endereco::Rede(endereco)) => Some(endereco),
        _ => None,
    }
}
