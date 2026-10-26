//! Quem insiste em parear depois de recusado fica de fora por um tempo.
//!
//! Com o pedido partindo do aparelho, a porta fica aberta enquanto o serviço roda, e cada pedido
//! põe uma janela na frente de quem está no computador. Um aparelho da rede que pedisse sem parar
//! viraria uma janela sem fim. Três pareamentos que não terminam — recusados, sem resposta ou
//! abandonados — deixam aquele endereço de fora por [`CASTIGO`].

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use tokio::time::Instant;

/// Quantos pareamentos sem fim um endereço pode ter antes de ficar de fora.
pub(crate) const TOLERADAS: u32 = 3;

/// Quanto tempo ele fica de fora — e quanto tempo uma recusa é lembrada.
pub(crate) const CASTIGO: Duration = Duration::from_secs(600);

/// As recusas de cada endereço.
#[derive(Debug, Default)]
pub(crate) struct Recusas(HashMap<IpAddr, (u32, Instant)>);

impl Recusas {
    /// Um pareamento deste endereço não terminou.
    pub(crate) fn anotar(&mut self, endereco: IpAddr, agora: Instant) {
        let (quantas, quando) = self.0.entry(endereco).or_insert((0, agora));
        if agora.saturating_duration_since(*quando) >= CASTIGO {
            *quantas = 0;
        }
        *quantas += 1;
        *quando = agora;
    }

    /// Pareou: o que passou não conta mais.
    pub(crate) fn esquecer(&mut self, endereco: IpAddr) {
        self.0.remove(&endereco);
    }

    /// Se o endereço está de fora agora.
    pub(crate) fn de_fora(&self, endereco: IpAddr, agora: Instant) -> bool {
        self.0.get(&endereco).is_some_and(|(quantas, quando)| {
            *quantas >= TOLERADAS && agora.saturating_duration_since(*quando) < CASTIGO
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(final_: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, final_])
    }

    #[test]
    fn a_terceira_recusa_deixa_de_fora_so_aquele_endereco() {
        let mut recusas = Recusas::default();
        let agora = Instant::now();
        for _ in 0..TOLERADAS - 1 {
            recusas.anotar(ip(5), agora);
        }
        assert!(!recusas.de_fora(ip(5), agora), "duas ainda é engano");
        recusas.anotar(ip(5), agora);
        assert!(recusas.de_fora(ip(5), agora));
        assert!(!recusas.de_fora(ip(6), agora), "os outros aparelhos seguem");
    }

    #[test]
    fn o_castigo_acaba_e_parear_apaga_o_passado() {
        let mut recusas = Recusas::default();
        let agora = Instant::now();
        for _ in 0..TOLERADAS {
            recusas.anotar(ip(5), agora);
        }
        assert!(!recusas.de_fora(ip(5), agora + CASTIGO));
        recusas.anotar(ip(5), agora + CASTIGO);
        assert!(
            !recusas.de_fora(ip(5), agora + CASTIGO),
            "a conta recomeçou"
        );
        recusas.esquecer(ip(5));
        assert!(!recusas.de_fora(ip(5), agora));
    }
}
