//! Por onde o par é alcançado em cada portador, e por quais há enlace de pé.
//!
//! Antes havia um endereço só e um "enlace de pé" só: o par era da rede **ou** do rádio. Com a rota
//! dupla ([ADR-0012](../../../docs/adr/0012-rota-dupla.md)) o serviço mantém um enlace em cada
//! portador, e cada um tem o seu endereço — o do rádio vem do pareamento, da configuração ou do
//! próprio par (`Control::Reach`); o da rede, do pareamento, do `peer_addr` ou da descoberta.
//!
//! Mora aqui, e não no serviço, pela razão do resto do crate: é sobre os dois portadores ao mesmo
//! tempo. Não disca nada — guarda o que se sabe e diz quando vale discar de novo.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use ir_proto::carrier::Carrier;

use crate::Endereco;

/// Quanto se espera uma discagem terminar antes de tentar de novo pelo mesmo portador.
///
/// Discar o rádio para um par fora de alcance bloqueia o endpoint pelo *page timeout* — segundos.
/// Uma discagem nova a cada rodada de 3 s empilharia pedidos mais depressa do que eles terminam.
pub const PRAZO_DA_DISCAGEM: Duration = Duration::from_secs(10);

/// De quanto em quanto tempo, no máximo, se procura o par na rede.
///
/// A busca é um *broadcast* de 2 s; repetida a cada rodada, viraria ruído na rede de todo mundo.
pub const INTERVALO_DA_BUSCA: Duration = Duration::from_secs(30);

/// Um portador visto daqui: se o enlace está de pé, e desde quando há discagem sem resposta.
#[derive(Debug, Clone, Copy, Default)]
struct Via {
    de_pe: bool,
    discando: Option<Instant>,
}

/// Por onde o par é alcançado, portador a portador.
#[derive(Debug, Clone, Default)]
pub struct Alcance {
    /// O endereço de rede escrito na configuração. Nunca é esquecido: foi alguém que escreveu.
    rede_configurada: Option<SocketAddr>,
    /// O endereço de rede aprendido — no pareamento, numa conexão, ou pela descoberta.
    rede: Option<SocketAddr>,
    radio: Option<Endereco>,
    via_rede: Via,
    via_radio: Via,
    /// Quando saiu a última busca do par na rede.
    buscou: Option<Instant>,
}

impl Alcance {
    /// O que se sabe na subida: o endereço escrito na configuração, e os gravados do par.
    pub fn novo(
        configurado: Option<Endereco>,
        gravados: impl IntoIterator<Item = Endereco>,
    ) -> Self {
        let mut alcance = Self::default();
        for endereco in gravados {
            alcance.anotar(endereco);
        }
        match configurado {
            Some(Endereco::Rede(rede)) => alcance.rede_configurada = Some(rede),
            Some(radio @ Endereco::Radio(_)) => alcance.anotar(radio),
            None => {}
        }
        alcance
    }

    /// Como [`Self::novo`], a partir dos endereços escritos como texto — na configuração, por
    /// exemplo. Texto que não é endereço é ignorado.
    pub fn dos_textos<'a>(
        configurado: Option<&str>,
        gravados: impl IntoIterator<Item = Option<&'a str>>,
    ) -> Self {
        let ler = |texto: Option<&str>| texto.and_then(Endereco::ler);
        Self::novo(ler(configurado), gravados.into_iter().filter_map(ler))
    }

    /// Anota onde o par está no portador deste endereço.
    pub const fn anotar(&mut self, endereco: Endereco) {
        match endereco {
            Endereco::Rede(rede) => self.rede = Some(rede),
            Endereco::Radio(_) => self.radio = Some(endereco),
        }
    }

    /// Onde discar o par por este portador, se se sabe.
    #[must_use]
    pub fn endereco(&self, portador: Carrier) -> Option<Endereco> {
        match portador {
            Carrier::Udp => self.rede.or(self.rede_configurada).map(Endereco::Rede),
            Carrier::Rfcomm => self.radio,
            Carrier::Tcp => None,
        }
    }

    const fn via(&self, portador: Carrier) -> Via {
        match portador {
            Carrier::Rfcomm => self.via_radio,
            Carrier::Udp | Carrier::Tcp => self.via_rede,
        }
    }

    const fn via_mut(&mut self, portador: Carrier) -> &mut Via {
        match portador {
            Carrier::Rfcomm => &mut self.via_radio,
            Carrier::Udp | Carrier::Tcp => &mut self.via_rede,
        }
    }

    /// O enlace por este portador ficou de pé.
    pub const fn subiu(&mut self, portador: Carrier) {
        *self.via_mut(portador) = Via {
            de_pe: true,
            discando: None,
        };
    }

    /// O enlace por este portador caiu, ou a discagem por ele não deu em nada.
    pub const fn caiu(&mut self, portador: Carrier) {
        *self.via_mut(portador) = Via {
            de_pe: false,
            discando: None,
        };
    }

    /// A discagem pela rede falhou: o endereço aprendido pode ter envelhecido (o DHCP deu outro), e
    /// a próxima rodada volta a procurar o par. O configurado fica.
    pub const fn rede_falhou(&mut self) {
        self.rede = None;
        self.caiu(Carrier::Udp);
    }

    /// Se o enlace por este portador está de pé.
    #[must_use]
    pub const fn de_pe(&self, portador: Carrier) -> bool {
        self.via(portador).de_pe
    }

    /// Os portadores com enlace de pé, na ordem de preferência.
    pub fn de_pe_agora(&self) -> impl Iterator<Item = Carrier> + '_ {
        [Carrier::Rfcomm, Carrier::Udp]
            .into_iter()
            .filter(|portador| self.de_pe(*portador))
    }

    /// Se há algum enlace de pé.
    #[must_use]
    pub fn algum_de_pe(&self) -> bool {
        self.de_pe_agora().next().is_some()
    }

    /// Esquece que havia enlaces de pé, sem esquecer onde o par está.
    pub const fn derrubar_todos(&mut self) {
        self.caiu(Carrier::Rfcomm);
        self.caiu(Carrier::Udp);
    }

    /// Se vale discar por este portador agora: sem enlace, e sem discagem recente esperando.
    #[must_use]
    pub fn pode_discar(&self, portador: Carrier, agora: Instant) -> bool {
        let via = self.via(portador);
        !via.de_pe
            && via
                .discando
                .is_none_or(|desde| agora.duration_since(desde) >= PRAZO_DA_DISCAGEM)
    }

    /// Saiu uma discagem por este portador.
    pub const fn discou(&mut self, portador: Carrier, agora: Instant) {
        self.via_mut(portador).discando = Some(agora);
    }

    /// Se vale procurar o par na rede agora, e anota a busca quando vale.
    pub fn buscar_agora(&mut self, agora: Instant) -> bool {
        let vale = self
            .buscou
            .is_none_or(|desde| agora.duration_since(desde) >= INTERVALO_DA_BUSCA);
        if vale {
            self.buscou = Some(agora);
        }
        vale
    }
}

impl core::fmt::Display for Alcance {
    /// Onde o par está em cada portador, e se o enlace está de pé — para o diagnóstico.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (indice, (nome, portador)) in [("rede", Carrier::Udp), ("rádio", Carrier::Rfcomm)]
            .into_iter()
            .enumerate()
        {
            let separador = if indice == 0 { "" } else { ", " };
            match self.endereco(portador) {
                Some(endereco) => write!(f, "{separador}{nome} {endereco}")?,
                None => write!(f, "{separador}{nome} desconhecido")?,
            }
            if self.de_pe(portador) {
                f.write_str(" (de pé)")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RADIO: &str = "AC:50:DE:47:EB:28";
    const REDE: &str = "10.0.0.135:52525";

    fn ler(texto: &str) -> Endereco {
        Endereco::ler(texto).expect("endereço válido")
    }

    #[test]
    fn os_enderecos_gravados_dao_um_por_portador() {
        let alcance = Alcance::novo(None, [ler(REDE), ler(RADIO)]);
        assert_eq!(alcance.endereco(Carrier::Udp), Some(ler(REDE)));
        assert_eq!(alcance.endereco(Carrier::Rfcomm), Some(ler(RADIO)));
    }

    #[test]
    fn o_endereco_de_rede_aprendido_que_falha_e_esquecido_e_o_configurado_fica() {
        let mut alcance = Alcance::novo(Some(ler("10.0.0.9:52525")), []);
        alcance.anotar(ler(REDE));
        assert_eq!(
            alcance.endereco(Carrier::Udp),
            Some(ler(REDE)),
            "o aprendido vence"
        );

        alcance.rede_falhou();

        assert_eq!(alcance.endereco(Carrier::Udp), Some(ler("10.0.0.9:52525")));
    }

    #[test]
    fn dos_textos_ignora_o_que_nao_e_endereco() {
        let alcance = Alcance::dos_textos(Some("lixo"), [Some(RADIO), None, Some("nada")]);
        assert_eq!(alcance.endereco(Carrier::Rfcomm), Some(ler(RADIO)));
        assert_eq!(alcance.endereco(Carrier::Udp), None);
    }

    #[test]
    fn um_endereco_de_radio_configurado_vale_como_radio() {
        let alcance = Alcance::novo(Some(ler(RADIO)), []);
        assert_eq!(alcance.endereco(Carrier::Rfcomm), Some(ler(RADIO)));
        assert_eq!(alcance.endereco(Carrier::Udp), None);
    }

    #[test]
    fn uma_discagem_recente_espera_o_prazo_e_um_enlace_de_pe_nao_disca() {
        let mut alcance = Alcance::default();
        let agora = Instant::now();
        assert!(alcance.pode_discar(Carrier::Rfcomm, agora));

        alcance.discou(Carrier::Rfcomm, agora);
        assert!(!alcance.pode_discar(Carrier::Rfcomm, agora + Duration::from_secs(3)));
        assert!(alcance.pode_discar(Carrier::Rfcomm, agora + PRAZO_DA_DISCAGEM));
        assert!(
            alcance.pode_discar(Carrier::Udp, agora),
            "cada portador tem o seu prazo"
        );

        alcance.subiu(Carrier::Rfcomm);
        assert!(!alcance.pode_discar(Carrier::Rfcomm, agora + PRAZO_DA_DISCAGEM));
        assert_eq!(
            alcance.de_pe_agora().collect::<Vec<_>>(),
            vec![Carrier::Rfcomm]
        );
    }

    #[test]
    fn o_diagnostico_diz_onde_e_o_que_esta_de_pe() {
        let mut alcance = Alcance::novo(None, [ler(REDE)]);
        alcance.subiu(Carrier::Udp);
        assert_eq!(
            alcance.to_string(),
            "rede 10.0.0.135:52525 (de pé), rádio desconhecido"
        );
    }

    #[test]
    fn a_busca_na_rede_tem_intervalo() {
        let mut alcance = Alcance::default();
        let agora = Instant::now();
        assert!(alcance.buscar_agora(agora));
        assert!(!alcance.buscar_agora(agora + Duration::from_secs(5)));
        assert!(alcance.buscar_agora(agora + INTERVALO_DA_BUSCA));
    }
}
