//! A troca de chaves de um enlace que está de pé.
//!
//! Depois de 2^20 quadros com as mesmas chaves, ou de [`IDADE_MAXIMA`]
//! ([03, §3](../../../docs/03-protocolo.md)), quem envia refaz o handshake — `Noise_IK`, com a chave
//! do par que já está fixada — e troca o enlace **sem derrubar a sessão**: nenhum aviso de queda
//! sai de nenhum dos dois lados.
//!
//! # Sem parar o enlace, e sem desalinhar os dois lados
//!
//! A primeira versão conduzia o handshake inteiro dentro do laço do endpoint e trocava as chaves
//! dos dois lados assim que cada um terminava a sua parte. Na bancada isso derrubou a conexão a
//! cada poucas horas, e numa delas para sempre:
//!
//! - **Quem pedia ficava surdo.** Enquanto esperava a resposta, descartava os quadros de dados
//!   que chegavam — e o prazo recomeçava a cada um descartado. Com o par movendo o cursor, nunca
//!   vencia: o endpoint parava de mandar, sem avisar queda, e o serviço achava a rede de pé.
//! - **Quem respondia trocava cedo demais.** Mandava a resposta e passava a cifrar com as chaves
//!   novas na hora. Se a resposta se perdia no Wi-Fi, cada lado ficava com um par de chaves, e
//!   nenhum quadro abria do outro lado até a próxima tentativa, um minuto depois.
//!
//! Agora o pedido sai e o laço segue; a resposta é tratada quando chegar, como qualquer datagrama,
//! e sem ela a tentativa vence em [`PRAZO_DA_RESPOSTA`] sem custo nenhum. E as chaves novas só
//! passam a cifrar quando se sabe que o outro lado também as tem: quem pediu, ao ler a resposta;
//! quem respondeu, ao abrir o primeiro quadro cifrado com elas. Até lá as anteriores continuam
//! abrindo o que chega ([`Reserva`]), então a troca nunca perde quadro.
//!
//! Um par que não conhece o modo de troca ignora o datagrama: a tentativa vence, o enlace atual
//! segue valendo, e a próxima vem depois de [`ESPERA_ENTRE_TENTATIVAS`].

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ir_crypto::enlace::{ConnectMode, Established, concluir};
use ir_crypto::{Handshake, PublicKey};
use tracing::{debug, info};

use super::{Endpoint, State};
use crate::link::SecureLink;
use crate::wire::{self, Kind, Mode};

/// Quanto se espera para tentar de novo quando a troca não se completou.
///
/// Uma tentativa que falha não custa nada ao enlace, então não há por que esperar muito.
pub(super) const ESPERA_ENTRE_TENTATIVAS: Duration = Duration::from_secs(10);

/// Quanto se espera a resposta a um pedido de troca antes de dá-lo por perdido.
pub(super) const PRAZO_DA_RESPOSTA: Duration = Duration::from_secs(2);

/// A idade máxima das chaves de um enlace, pelo que vier primeiro com o contador
/// ([03, §3.1](../../../docs/03-protocolo.md)).
pub(super) const IDADE_MAXIMA: Duration = Duration::from_secs(10 * 60);

/// Por quanto tempo as chaves substituídas ainda abrem o que chega: o que já estava no ar quando o
/// par trocou, e o que ele cifra até saber da troca.
pub(super) const VIDA_DAS_ANTERIORES: Duration = Duration::from_secs(30);

/// Se é este lado quem troca as chaves por idade.
///
/// Só um dos dois, e sempre o mesmo: as duas pontas fazem dez minutos quase juntas, e dois pedidos
/// de troca cruzados se atropelariam. Quem tem a menor chave pública pede — a chave do par
/// [precede](PublicKey::precede) a nossa —; a outra ponta atende.
pub(super) fn troca_por_idade(minha: &PublicKey, do_par: &PublicKey, idade: Duration) -> bool {
    idade >= IDADE_MAXIMA && do_par.precede(minha)
}

/// Um pedido de troca que saiu daqui e espera a resposta.
pub(super) struct Pendente {
    handshake: Handshake,
    desde: Instant,
}

/// Um segundo jogo de chaves, além do que cifra o que sai.
#[derive(Debug)]
pub(super) struct Reserva {
    link: SecureLink,
    papel: Papel,
    desde: Instant,
}

/// Por que a [`Reserva`] existe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Papel {
    /// As chaves que este lado respondeu, à espera do primeiro quadro do par cifrado com elas.
    Proxima,
    /// As chaves substituídas, abrindo o que o par ainda cifrou com elas.
    Anterior,
}

impl Reserva {
    /// Numa caixa: um enlace a mais deixaria todo estado do endpoint do tamanho de dois.
    fn nova(link: SecureLink, papel: Papel) -> Box<Self> {
        Box::new(Self {
            link,
            papel,
            desde: Instant::now(),
        })
    }

    /// As anteriores têm prazo; as próximas valem até a próxima troca as substituir.
    fn venceu(&self) -> bool {
        self.papel == Papel::Anterior && self.desde.elapsed() >= VIDA_DAS_ANTERIORES
    }
}

/// Abre um datagrama de dados pelas chaves em uso, ou pela reserva.
///
/// Abrir pelas [próximas](Papel::Proxima) é a prova de que o par já cifra com elas: a troca se
/// confirma aqui, e elas passam a cifrar o que sai.
pub(super) fn abrir_com_reserva(
    link: &mut SecureLink,
    reserva: &mut Option<Box<Reserva>>,
    datagram: &[u8],
) -> Option<(Kind, Vec<u8>)> {
    if let Ok(aberto) = link.open(datagram) {
        return Some(aberto);
    }
    if reserva.as_ref().is_some_and(|guardada| guardada.venceu()) {
        *reserva = None;
    }
    let guardada = reserva.as_mut()?;
    let aberto = guardada.link.open(datagram).ok()?;
    if guardada.papel == Papel::Proxima
        && let Some(proxima) = reserva.take()
    {
        let anterior = core::mem::replace(link, proxima.link);
        *reserva = Some(Reserva::nova(anterior, Papel::Anterior));
        info!("chaves do enlace trocadas");
    }
    Some(aberto)
}

impl Endpoint {
    /// Pede a troca de chaves se o enlace pedir, se não há pedido no ar e se não houve tentativa
    /// recente. Não espera a resposta: ela chega por [`Self::atender_rechave`].
    pub(super) async fn rechavear_se_preciso(&mut self) {
        if self
            .rechave
            .as_ref()
            .is_some_and(|pendente| pendente.desde.elapsed() >= PRAZO_DA_RESPOSTA)
        {
            self.rechave = None;
            debug!("o par não respondeu à troca de chaves; o enlace atual segue");
        }
        let State::Established {
            link, peer_static, ..
        } = &self.state
        else {
            return;
        };
        let por_idade = troca_por_idade(&self.identity.public(), peer_static, link.idade());
        if self.rechave.is_some()
            || !(link.should_rekey() || por_idade)
            || self
                .rechave_tentada
                .is_some_and(|quando| quando.elapsed() < ESPERA_ENTRE_TENTATIVAS)
        {
            return;
        }
        let (peer, chave) = (link.peer(), *peer_static);
        self.rechave_tentada = Some(Instant::now());
        let pedido = ConnectMode::Rekey(chave)
            .iniciar(&self.identity)
            .and_then(|mut handshake| Ok((handshake.write_message()?, handshake)));
        let Ok((mensagem, handshake)) = pedido else {
            return;
        };
        let datagrama = wire::handshake_datagram(Mode::Rekey, &mensagem);
        if self.socket.send_to(&datagrama, peer).await.is_ok() {
            self.rechave = Some(Pendente {
                handshake,
                desde: Instant::now(),
            });
        }
    }

    /// Um datagrama de troca de chaves do par: a resposta ao nosso pedido, ou um pedido dele.
    /// Devolve `true` se o datagrama era isso.
    pub(super) async fn atender_rechave(&mut self, from: SocketAddr, datagram: &[u8]) -> bool {
        let State::Established {
            link, peer_static, ..
        } = &self.state
        else {
            return false;
        };
        let Some((Mode::Rekey, mensagem)) = wire::parse_handshake(datagram) else {
            return false;
        };
        if from != link.peer() {
            return false;
        }
        let chave = *peer_static;
        // Se não for a resposta, é um pedido do par cruzado com o nosso: o nosso fica para a
        // próxima tentativa, e o dele é atendido.
        if let Some(mut pendente) = self.rechave.take()
            && pendente.handshake.read_message(mensagem).is_ok()
        {
            if let Ok(novo) = concluir(pendente.handshake) {
                self.trocar_ja(chave, novo);
            }
            return true;
        }
        self.responder_rechave(from, chave, mensagem).await;
        true
    }

    /// Responde ao pedido do par e guarda as chaves novas como [próximas](Papel::Proxima): o que
    /// sai continua com as atuais até o par mostrar que recebeu a resposta.
    async fn responder_rechave(&mut self, from: SocketAddr, chave: PublicKey, mensagem: &[u8]) {
        let resposta = Mode::Rekey
            .responder(&self.identity)
            .and_then(|mut handshake| {
                handshake.read_message(mensagem)?;
                let resposta = handshake.write_message()?;
                Ok((resposta, concluir(handshake)?))
            });
        // Falha aqui é lixo ou repetição; outra chave seria outra máquina se fazendo passar pelo
        // par. Nos dois casos o enlace atual continua.
        let Ok((resposta, novo)) = resposta else {
            return;
        };
        if novo.peer_static != chave {
            return;
        }
        let datagrama = wire::handshake_datagram(Mode::Rekey, &resposta);
        if self.socket.send_to(&datagrama, from).await.is_err() {
            return;
        }
        if let State::Established { reserva, .. } = &mut self.state {
            let link = SecureLink::new(Arc::clone(&self.socket), from, novo.transport);
            *reserva = Some(Reserva::nova(link, Papel::Proxima));
            debug!("troca de chaves respondida; as novas valem quando o par as usar");
        }
    }

    /// A resposta ao nosso pedido chegou: o par já tem as chaves novas, e elas passam a cifrar o
    /// que sai. As anteriores ficam abrindo o que ele cifrou antes de saber.
    fn trocar_ja(&mut self, chave: PublicKey, novo: Established) {
        if novo.peer_static != chave {
            return;
        }
        let State::Established { link, reserva, .. } = &mut self.state else {
            return;
        };
        let novo = SecureLink::new(Arc::clone(&self.socket), link.peer(), novo.transport);
        let anterior = core::mem::replace(link, novo);
        *reserva = Some(Reserva::nova(anterior, Papel::Anterior));
        self.rechave_tentada = None;
        info!("chaves do enlace trocadas");
    }
}

#[cfg(test)]
mod testes;
