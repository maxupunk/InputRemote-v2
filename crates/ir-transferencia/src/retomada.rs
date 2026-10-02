//! A cópia que o enlace interrompeu: espera a conexão voltar e recomeça sozinha.
//!
//! Antes, uma queda do canal de arquivos no meio de uma cópia era o fim dela. O cartão dizia "a
//! cópia não atravessou", e a pessoa tinha de copiar de novo — mesmo quando o Wi-Fi voltava em dois
//! segundos. Agora quem envia guarda a cópia na fila ([`crate::fila::Fila::devolver`]), e ela
//! recomeça quando o enlace voltar; os dois lados mostram a mesma espera. Com dois limites, porque
//! esperar para sempre também é falhar, só que calado:
//!
//! - [`PRAZO_PARA_VOLTAR`]: o tempo que a conexão tem para voltar; depois, a cópia desiste e diz
//!   por quê;
//! - [`MAX_QUEDAS`]: uma cópia que derruba o enlace toda vez não é tentada para sempre.
//!
//! Recomeça do começo, e não de onde parou, pela mesma razão de o `ir-files` não retomar: a
//! garantia pedida é cópia certa, e a montagem do outro lado se apaga na queda.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Motivo, Sentido};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::fila::{Devolucao, Fila, Trabalho};
use crate::sessao::{anunciar, anunciar_queda};

/// Quanto a conexão de arquivos tem para voltar antes de a cópia que espera desistir.
///
/// Um Wi-Fi que cochilou ou trocou de ponto de acesso volta em segundos; um par que reinicia o
/// serviço, em menos de meio minuto — a discagem tenta a cada três. Um minuto cobre os dois com
/// folga, e não deixa uma cópia "esperando" depois de a pessoa já ter desistido dela.
pub(crate) const PRAZO_PARA_VOLTAR: Duration = Duration::from_secs(60);

/// Quantas quedas no meio da mesma cópia até ela desistir.
///
/// Uma queda é a rede; três seguidas na mesma cópia já dizem que alguma coisa nela derruba o enlace,
/// e tentar de novo só repetiria a queda.
pub(crate) const MAX_QUEDAS: u8 = 3;

/// Por onde a interface fica sabendo.
type Avisos = broadcast::Sender<Aviso>;

/// O envio caiu no meio: a cópia volta para a fila, ou desiste depois de [`MAX_QUEDAS`].
///
/// Quem mostra a espera não é esta função, e sim quem conduz o canal ao ver o enlace cair
/// ([`mostrar_espera`]): é o mesmo lugar que mostra a de uma cópia pedida com o enlace já caído.
pub(crate) fn envio_caiu(
    fila: &Fila,
    avisos: &Avisos,
    mut trabalho: Trabalho,
    (nome, progresso): (&str, (u64, u64)),
) {
    trabalho.quedas = trabalho.quedas.saturating_add(1);
    trabalho.progresso = progresso;
    if trabalho.quedas >= MAX_QUEDAS {
        fila.terminou();
        warn!(
            quedas = trabalho.quedas,
            "o enlace caiu de novo no meio da mesma cópia; ela desiste"
        );
        anunciar_queda(avisos, Sentido::Enviando, Some((nome, progresso)));
        return;
    }
    match fila.devolver(trabalho) {
        Devolucao::VaiDeNovo => {
            info!("o enlace caiu no meio da cópia; ela recomeça quando ele voltar");
        }
        Devolucao::Cancelada => {
            let fase = Fase::Parada(Motivo::Cancelada);
            anunciar(avisos, Sentido::Enviando, nome, progresso, fase);
        }
    }
}

/// Mostra na tela a cópia que espera o enlace, e marca o prazo dela.
///
/// Não faz nada se não há cópia esperando, ou se ela já está à vista: o aviso e o prazo correm uma
/// vez por espera.
pub(crate) fn mostrar_espera(fila: &Arc<Fila>, avisos: &Avisos) {
    let Some(espera) = fila.mostrar_espera() else {
        return;
    };
    let nome = ir_files::publicacao::nome_do_pedido(&espera.caminhos);
    info!("a cópia espera a conexão de arquivos voltar");
    let fase = Fase::AguardandoConexao;
    anunciar(avisos, Sentido::Enviando, &nome, espera.progresso, fase);
    let (fila, avisos) = (Arc::clone(fila), avisos.clone());
    depois_do_prazo(move || {
        if fila.desistir(&espera).is_some() {
            warn!("a conexão de arquivos não voltou no prazo; a cópia que esperava desiste");
            anunciar_queda(&avisos, Sentido::Enviando, Some((&nome, espera.progresso)));
        }
    });
}

/// Conta que a cópia que esperava a conexão na tela saiu da fila por decisão da pessoa — cancelou,
/// ou copiou outra coisa.
pub(crate) fn espera_cancelada(avisos: &Avisos, trabalho: &Trabalho) {
    let nome = ir_files::publicacao::nome_do_pedido(&trabalho.caminhos);
    let fase = Fase::Parada(Motivo::Cancelada);
    anunciar(avisos, Sentido::Enviando, &nome, trabalho.progresso, fase);
}

/// As recepções que já começaram, para a espera de uma interrompida saber se o par recomeçou.
///
/// Quem recebe não guarda a cópia — quem a tem é o outro lado. O que ele sabe é que uma chegava e
/// parou; se nenhuma outra começar no prazo, ela não vem mais.
#[derive(Debug, Default)]
pub(crate) struct Recepcoes(AtomicU64);

impl Recepcoes {
    /// Uma recepção começou: a espera de qualquer interrompida acabou.
    pub(crate) fn comecou(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    /// A recepção caiu no meio: mostra a espera e, se o par não recomeçar no prazo, diz que a cópia
    /// não atravessou.
    pub(crate) fn caiu(self: &Arc<Self>, avisos: &Avisos, nome: &str, progresso: (u64, u64)) {
        let fase = Fase::AguardandoConexao;
        anunciar(avisos, Sentido::Recebendo, nome, progresso, fase);
        let marca = self.0.load(Ordering::Relaxed);
        let (recepcoes, avisos, nome) = (Arc::clone(self), avisos.clone(), nome.to_owned());
        depois_do_prazo(move || {
            if recepcoes.0.load(Ordering::Relaxed) == marca {
                anunciar_queda(&avisos, Sentido::Recebendo, Some((&nome, progresso)));
            }
        });
    }
}

/// Faz `acao` depois de [`PRAZO_PARA_VOLTAR`], fora de quem pediu.
///
/// Sem *runtime* — o serviço saindo, e uma recepção largada no `Drop` —, não há prazo a contar
/// nem tela a avisar: não faz nada, em vez de entrar em pânico.
fn depois_do_prazo(acao: impl FnOnce() + Send + 'static) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    runtime.spawn(async move {
        tokio::time::sleep(PRAZO_PARA_VOLTAR).await;
        acao();
    });
}

#[cfg(test)]
mod testes;
