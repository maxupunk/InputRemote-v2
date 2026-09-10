//! Por onde a conexão passa, e de que lado fica a outra tela.

use serde::{Deserialize, Serialize};

use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;

/// Por onde a conexão está passando, com o nome que o usuário conhece.
///
/// "RFCOMM" é o nome certo e não diz nada a ninguém; "Bluetooth" é o que está escrito na caixa do
/// adaptador. O termo técnico aparece no diagnóstico, onde serve para alguma coisa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Portador {
    /// Bluetooth.
    Bluetooth,
    /// Rede local, para teclado e mouse.
    RedeLocal,
    /// Rede local, para imagens e arquivos.
    RedeDeArquivos,
}

impl Portador {
    /// O nome que aparece na tela.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Bluetooth => "Bluetooth",
            Self::RedeLocal => "Rede local",
            Self::RedeDeArquivos => "Rede local (arquivos)",
        }
    }

    /// O nome técnico, que só aparece no diagnóstico.
    #[must_use]
    pub const fn nome_tecnico(self) -> &'static str {
        match self {
            Self::Bluetooth => "RFCOMM",
            Self::RedeLocal => "UDP",
            Self::RedeDeArquivos => "TCP",
        }
    }

    /// Se este portador serve para teclado e mouse.
    ///
    /// A interface usa para não oferecer ao usuário uma escolha que o produto vai recusar.
    #[must_use]
    pub const fn serve_para_entrada(self) -> bool {
        matches!(self, Self::Bluetooth | Self::RedeLocal)
    }

    /// O portador do protocolo correspondente.
    #[must_use]
    pub const fn no_protocolo(self) -> Carrier {
        match self {
            Self::Bluetooth => Carrier::Rfcomm,
            Self::RedeLocal => Carrier::Udp,
            Self::RedeDeArquivos => Carrier::Tcp,
        }
    }
}

impl From<Carrier> for Portador {
    fn from(portador: Carrier) -> Self {
        match portador {
            Carrier::Rfcomm => Self::Bluetooth,
            Carrier::Udp => Self::RedeLocal,
            Carrier::Tcp => Self::RedeDeArquivos,
        }
    }
}

/// De que lado fica a outra tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Borda {
    /// À esquerda.
    Esquerda,
    /// À direita.
    Direita,
    /// Acima.
    Acima,
    /// Abaixo.
    Abaixo,
}

impl Borda {
    /// Todas, na ordem em que a tela as desenha.
    pub const TODAS: [Self; 4] = [Self::Esquerda, Self::Direita, Self::Acima, Self::Abaixo];

    /// Como a frase da tela se refere a esta borda.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Esquerda => "esquerda",
            Self::Direita => "direita",
            Self::Acima => "de cima",
            Self::Abaixo => "de baixo",
        }
    }

    /// A borda do protocolo correspondente.
    #[must_use]
    pub const fn no_protocolo(self) -> Edge {
        match self {
            Self::Esquerda => Edge::Left,
            Self::Direita => Edge::Right,
            Self::Acima => Edge::Top,
            Self::Abaixo => Edge::Bottom,
        }
    }
}

impl From<Edge> for Borda {
    fn from(borda: Edge) -> Self {
        match borda {
            Edge::Left => Self::Esquerda,
            Edge::Right => Self::Direita,
            Edge::Top => Self::Acima,
            Edge::Bottom => Self::Abaixo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_portadores_sobrevivem_a_ida_e_volta() {
        for portador in [Carrier::Rfcomm, Carrier::Udp, Carrier::Tcp] {
            assert_eq!(Portador::from(portador).no_protocolo(), portador);
        }
    }

    #[test]
    fn as_bordas_sobrevivem_a_ida_e_volta() {
        for borda in Edge::ALL {
            assert_eq!(Borda::from(borda).no_protocolo(), borda);
        }
        for borda in Borda::TODAS {
            assert_eq!(Borda::from(borda.no_protocolo()), borda);
        }
    }

    #[test]
    fn a_tela_nao_ve_o_nome_tecnico_do_portador() {
        // Se "RFCOMM" ou "UDP" vazar para `nome()`, a tela passa a falar uma língua que o usuário
        // não fala. O nome técnico existe, e o lugar dele é o diagnóstico.
        for portador in [
            Portador::Bluetooth,
            Portador::RedeLocal,
            Portador::RedeDeArquivos,
        ] {
            let nome = portador.nome();
            assert!(!nome.contains("RFCOMM"), "{nome}");
            assert!(!nome.contains("UDP"), "{nome}");
            assert!(!nome.contains("TCP"), "{nome}");
            assert!(!portador.nome_tecnico().is_empty());
        }
    }

    #[test]
    fn tcp_nao_serve_para_entrada() {
        // TCP não carrega teclado e mouse (docs/03 §1). A interface consulta isto para não oferecer
        // uma escolha que o serviço vai recusar.
        assert!(!Portador::RedeDeArquivos.serve_para_entrada());
        assert!(Portador::Bluetooth.serve_para_entrada());
        assert!(Portador::RedeLocal.serve_para_entrada());
    }
}
