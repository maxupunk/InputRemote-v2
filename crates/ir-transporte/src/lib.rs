//! O portador visto pelo serviço: uma porta só para os dois transportes.
//!
//! Antes deste módulo, o ator falava direto com `ir_net::NetCommand` e mandava **tudo** por ele
//! — inclusive os comandos que a sessão marcava como sendo do Bluetooth. O portador chegava em
//! `Command::Send { carrier, .. }` e era descartado no caminho. Enquanto só existia um
//! transporte isso não aparecia; com dois, seria um defeito silencioso do tipo mais caro: a
//! interface diria "Bluetooth" e os bytes iriam pela rede.
//!
//! # A inversão de dependência
//!
//! O ator passa a depender de [`Transporte`], e não de um crate de transporte. Quem implementa
//! são adaptadores finos: um sobre o `ir-net`, outro sobre o `ir-bt`. Acrescentar o TCP de
//! arquivos, depois, é acrescentar um terceiro adaptador — e nada no ator muda.
//!
//! # O que este módulo **não** faz
//!
//! Escolher portador. Essa política é única e mora no `ir-session`
//! ([`CarrierSet::pick_input_carrier`](ir_session::CarrierSet)), junto com o motivo da escolha,
//! que vai para a tela. O v1 tinha três políticas de degradação diferentes, uma por modo, e por
//! isso ninguém conseguia prever o comportamento ([00, §6](../../../docs/00-licoes-do-v1.md)).
//! Aqui o serviço só **roteia** o que a sessão já decidiu, e **relata** o que cada portador diz
//! de si.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod alcance;
pub mod dados;
pub mod descoberta;
mod radio;
mod rede;
mod subida;

pub use self::alcance::Alcance;
pub use self::dados::{Destinatario, EnlaceDeDados, Porta, Remetente};
pub use self::descoberta::{Descoberta, Encontrado};
pub use self::radio::{Pareados, Radio};
pub use self::rede::Rede;
pub use self::subida::{Abertos, RadioAberto, Reabridor, abrir, nome_da_maquina};

use std::net::SocketAddr;

use ir_bt::BdAddr;
use ir_crypto::PublicKey;
use ir_proto::carrier::Carrier;
use ir_proto::ids::{MachineId, RadioAddress};

/// O identificador de máquina de uma chave: os 16 primeiros bytes dela.
///
/// Um lugar só: o serviço, a descoberta e o canal de arquivos derivavam cada um o seu, e é por
/// esse número que a descoberta reconhece o par fixado na rede.
#[must_use]
pub fn maquina_da_chave(chave: &PublicKey) -> MachineId {
    let mut bytes = [0u8; 16];
    if let Some(inicio) = chave.0.get(..16) {
        bytes.copy_from_slice(inicio);
    }
    MachineId(bytes)
}

/// Onde um par pode ser alcançado.
///
/// Os dois portadores de entrada endereçam de formas incompatíveis — um `ip:porta`, um endereço
/// de rádio —, e é justamente por isso que o endereço carrega consigo o portador a que pertence.
/// Um `String` solto obrigaria cada ponto de uso a adivinhar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endereco {
    /// Um par na rede local.
    Rede(SocketAddr),
    /// Um par ao alcance do rádio.
    Radio(BdAddr),
}

impl Endereco {
    /// Por qual portador se fala com este endereço.
    pub const fn portador(self) -> Carrier {
        match self {
            Self::Rede(_) => Carrier::Udp,
            Self::Radio(_) => Carrier::Rfcomm,
        }
    }

    /// O endereço de rádio que o par contou em `Control::Reach`, pronto para discar.
    #[must_use]
    pub const fn do_radio(radio: RadioAddress) -> Self {
        Self::Radio(BdAddr(radio.0))
    }

    /// Lê um endereço escrito como texto.
    ///
    /// As duas formas não se confundem: `10.0.0.135:52525` tem dois grupos separados por `:` e
    /// nunca é um endereço de rádio; `AC:50:DE:47:EB:28` tem seis e nunca é um `ip:porta`. É o
    /// que permite a interface continuar mandando
    /// [`Pedido::IniciarPareamento`](ir_ipc::Pedido) com um texto só, sem ganhar vocabulário
    /// novo por causa do Bluetooth.
    pub fn ler(texto: &str) -> Option<Self> {
        let limpo = texto.trim();
        if let Ok(radio) = limpo.parse::<BdAddr>() {
            return Some(Self::Radio(radio));
        }
        limpo.parse::<SocketAddr>().ok().map(Self::Rede)
    }
}

impl core::fmt::Display for Endereco {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Rede(endereco) => write!(f, "{endereco}"),
            Self::Radio(endereco) => write!(f, "{endereco}"),
        }
    }
}

/// O que o serviço pede a um transporte.
///
/// Deliberadamente pequeno: é o mínimo que os dois transportes têm em comum, e cada método
/// existe porque o ator precisa dele. Nada aqui decide coisa alguma — decidir é da sessão.
pub trait Transporte: Send + Sync {
    /// Qual portador este transporte é.
    fn portador(&self) -> Carrier;

    /// Comece a falar com este par.
    ///
    /// Com `chave`, é reconexão à identidade fixada — e a conexão é recusada se o par apresentar
    /// outra. Sem ela, é o primeiro pareamento, que termina no código de seis dígitos.
    fn conectar(&self, alvo: Endereco, chave: Option<PublicKey>);

    /// Mande este quadro (bytes já codificados de `ir_proto::Frame`).
    fn enviar(&self, bytes: Vec<u8>);

    /// O usuário respondeu à comparação de códigos.
    fn confirmar_pareamento(&self, conferiu: bool);

    /// Se um pedido de pareamento que chega de fora deve ser atendido.
    fn aceitar_pareamento(&self, aceitar: bool);

    /// Encerre o enlace atual.
    fn desconectar(&self);
}

/// O que um transporte conta ao serviço.
///
/// **Todo fato carrega o portador que o produziu.** É o que permite ao ator alimentar
/// `Input::CarrierUp`/`CarrierDown` com o portador de verdade, em vez do `Carrier::Udp` fixo que
/// ele usava quando só havia um transporte — e é por isso que a tela podia dizer "Bluetooth
/// indisponível" mesmo com o rádio ligado dos dois lados.
#[derive(Debug)]
pub enum Fato {
    /// O handshake de pareamento terminou; aqui estão os dígitos para o usuário comparar.
    CodigoDePareamento {
        /// Por onde.
        portador: Carrier,
        /// Os seis dígitos.
        digitos: [u8; 6],
        /// A chave estática que o par apresentou, para gravar após a confirmação.
        chave_do_par: PublicKey,
        /// De onde ele falou.
        de: Endereco,
    },
    /// O enlace está pronto: pareamento confirmado dos dois lados, ou reconexão fixada.
    Estabelecido {
        /// Por onde.
        portador: Carrier,
        /// A chave estática do par.
        chave_do_par: PublicKey,
        /// De onde ele falou.
        de: Endereco,
    },
    /// Chegou um quadro do par.
    Quadro {
        /// Por onde.
        portador: Carrier,
        /// Os bytes, ainda por decodificar — e a decodificação confere o limite **deste**
        /// portador, que é menor no rádio que na rede.
        bytes: Vec<u8>,
    },
    /// O enlace caiu.
    Caiu {
        /// Por onde.
        portador: Carrier,
        /// O motivo curto e estável, para o registro e para a máquina de estados.
        motivo: String,
    },
    /// Uma falha que não derruba o transporte.
    Erro {
        /// Por onde.
        portador: Carrier,
        /// A frase que a tela mostra, já com o que a pessoa pode fazer quando há o que fazer.
        mensagem: String,
    },
    /// O transporte deixou de existir: o rádio foi desligado ou removido.
    ///
    /// Quem o abriu ([`Reabridor`]) volta a tentar em segundo plano; o ator o descarta.
    Perdido {
        /// Qual.
        portador: Carrier,
        /// Por quê.
        motivo: String,
    },
}

impl Fato {
    /// O portador que produziu este fato.
    pub const fn portador(&self) -> Carrier {
        match self {
            Self::CodigoDePareamento { portador, .. }
            | Self::Estabelecido { portador, .. }
            | Self::Quadro { portador, .. }
            | Self::Caiu { portador, .. }
            | Self::Erro { portador, .. }
            | Self::Perdido { portador, .. } => *portador,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RADIO: &str = "AC:50:DE:47:EB:28";
    const REDE: &str = "10.0.0.135:52525";

    #[test]
    fn os_dois_formatos_de_endereco_nao_se_confundem() {
        // É o que permite a interface mandar um texto só. Se um deles fosse lido como o outro, o
        // serviço discaria pelo portador errado — e a tela diria uma coisa e o produto faria
        // outra.
        assert_eq!(
            Endereco::ler(RADIO).expect("endereço de rádio"),
            Endereco::Radio(BdAddr([0xAC, 0x50, 0xDE, 0x47, 0xEB, 0x28]))
        );
        assert!(matches!(
            Endereco::ler(REDE).expect("endereço de rede"),
            Endereco::Rede(_)
        ));
    }

    #[test]
    fn cada_endereco_sabe_o_proprio_portador() {
        assert_eq!(
            Endereco::ler(RADIO).expect("rádio").portador(),
            Carrier::Rfcomm
        );
        assert_eq!(Endereco::ler(REDE).expect("rede").portador(), Carrier::Udp);
    }

    #[test]
    fn o_texto_sobrevive_a_ida_e_volta() {
        // O endereço volta para a configuração como texto; se mudasse de forma no caminho, uma
        // reconexão depois de reiniciar discaria para outro lugar.
        for texto in [RADIO, REDE] {
            let endereco = Endereco::ler(texto).expect("lê");
            assert_eq!(endereco.to_string(), texto);
            assert_eq!(Endereco::ler(&endereco.to_string()), Some(endereco));
        }
    }

    #[test]
    fn o_radio_contado_pelo_par_vira_endereco_de_radio() {
        let radio = RadioAddress([0xAC, 0x50, 0xDE, 0x47, 0xEB, 0x28]);
        assert_eq!(
            Endereco::do_radio(radio),
            Endereco::ler(RADIO).expect("rádio")
        );
        assert_eq!(Endereco::do_radio(radio).to_string(), radio.to_string());
    }

    #[test]
    fn um_endereco_sem_sentido_e_recusado() {
        for texto in ["", "   ", "nada", "10.0.0.135", "AC:50:DE:47:EB", "::"] {
            assert_eq!(Endereco::ler(texto), None, "{texto} não é endereço");
        }
    }

    #[test]
    fn espaco_em_volta_nao_atrapalha() {
        // O endereço pode vir de um campo digitado à mão.
        assert_eq!(Endereco::ler("  AC:50:DE:47:EB:28  "), Endereco::ler(RADIO));
    }

    #[test]
    fn todo_fato_diz_por_qual_portador_veio() {
        // A propriedade que faz o resto funcionar: sem ela o ator não teria como alimentar a
        // sessão com o portador certo, que é a origem do defeito que este trabalho corrige.
        let chave = PublicKey([7; 32]);
        let de = Endereco::ler(RADIO).expect("rádio");
        let fatos = [
            Fato::CodigoDePareamento {
                portador: Carrier::Rfcomm,
                digitos: [1, 2, 3, 4, 5, 6],
                chave_do_par: chave,
                de,
            },
            Fato::Estabelecido {
                portador: Carrier::Rfcomm,
                chave_do_par: chave,
                de,
            },
            Fato::Quadro {
                portador: Carrier::Rfcomm,
                bytes: vec![0],
            },
            Fato::Caiu {
                portador: Carrier::Rfcomm,
                motivo: "o par encerrou o canal".to_owned(),
            },
            Fato::Erro {
                portador: Carrier::Rfcomm,
                mensagem: "falhou".to_owned(),
            },
        ];
        for fato in fatos {
            assert_eq!(fato.portador(), Carrier::Rfcomm, "{fato:?}");
        }
    }
}
