//! Versão do protocolo e negociação.
//!
//! Toda a compatibilidade do produto passa por aqui. Como o formato de fio não é
//! autodescritivo (`docs/03-protocolo.md` §9), não existe tolerância no decodificador: ou
//! as duas pontas concordam na versão, ou a sessão não estabelece.

use serde::{Deserialize, Serialize};

use crate::error::{ProtoError, Result};

/// Versão do protocolo falada por esta build.
///
/// **Incrementar sempre que qualquer tipo de `ir-proto` mudar** — inclusive uma
/// reordenação de campos, que o `postcard` não detecta. Os vetores gravados em
/// `tests/vectors.rs` falham se isto for esquecido.
///
/// Versão 2: a sessão passou a tratar **todo** portador de entrada como datagrama — confirmação,
/// retransmissão e descarte de repetição valem também sobre RFCOMM —, que é o que permite a rota
/// dupla trocar de portador sem refazer a sessão ([ADR-0012](../../../docs/adr/0012-rota-dupla.md)).
/// Ganhou também [`Control::Reach`](crate::message::Control::Reach).
pub const CURRENT: ProtocolVersion = ProtocolVersion(2);

/// Versão mais antiga que esta build ainda aceita conversar.
///
/// Elevar isto abandona pares antigos de propósito, e é uma decisão de lançamento.
///
/// Subiu junto com [`CURRENT`], e não por descuido: uma ponta da versão 1 sobre RFCOMM não
/// confirma nada, e a janela de retransmissão desta encheria até derrubar a sessão a cada
/// segundo — em silêncio, parecendo defeito de rádio. Recusar na negociação diz o motivo. Não há
/// versão 1 lançada com quem manter compatibilidade (`tests/vectors/main.rs`, exceção de
/// pré-lançamento).
pub const MIN_SUPPORTED: ProtocolVersion = ProtocolVersion(2);

/// Versão do protocolo, monotônica.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtocolVersion(pub u16);

impl ProtocolVersion {
    /// O número cru, para exibir ou registrar.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl core::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// Resultado de uma negociação bem-sucedida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Negotiated {
    /// Versão que as duas pontas vão falar.
    pub version: ProtocolVersion,
    /// `true` quando a outra ponta é mais nova que esta build.
    ///
    /// Útil para a interface sugerir atualização, sem impedir o uso.
    pub remote_is_newer: bool,
}

/// Decide qual versão as duas pontas vão falar.
///
/// A regra é a menor das duas, desde que ela não seja anterior a [`MIN_SUPPORTED`].
/// Uma ponta mais nova conversa com uma mais velha rebaixando-se; uma ponta velha demais
/// é recusada, com o motivo dito.
///
/// # Errors
///
/// [`ProtoError::IncompatibleVersion`] quando a versão remota é anterior a
/// [`MIN_SUPPORTED`] — não há denominador comum.
pub fn negotiate(remote: ProtocolVersion) -> Result<Negotiated> {
    if remote < MIN_SUPPORTED {
        return Err(ProtoError::IncompatibleVersion {
            local: CURRENT.get(),
            remote: remote.get(),
        });
    }
    Ok(Negotiated {
        version: if remote < CURRENT { remote } else { CURRENT },
        remote_is_newer: remote > CURRENT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_version_negotiates_to_itself() {
        let n = negotiate(CURRENT).unwrap();
        assert_eq!(n.version, CURRENT);
        assert!(!n.remote_is_newer);
    }

    #[test]
    fn newer_remote_makes_us_stay_on_ours() {
        let newer = ProtocolVersion(CURRENT.get() + 5);
        let n = negotiate(newer).unwrap();
        assert_eq!(n.version, CURRENT, "não podemos falar o que não conhecemos");
        assert!(n.remote_is_newer);
    }

    #[test]
    fn older_but_supported_remote_makes_us_step_down() {
        // Só é exercível quando MIN_SUPPORTED < CURRENT; hoje são iguais, e o teste
        // documenta a regra para quando deixarem de ser.
        if MIN_SUPPORTED < CURRENT {
            let n = negotiate(MIN_SUPPORTED).unwrap();
            assert_eq!(n.version, MIN_SUPPORTED);
            assert!(!n.remote_is_newer);
        }
    }

    #[test]
    fn too_old_remote_is_refused_with_both_numbers() {
        let ancient = ProtocolVersion(MIN_SUPPORTED.get().saturating_sub(1));
        if ancient < MIN_SUPPORTED {
            let err = negotiate(ancient).unwrap_err();
            assert_eq!(
                err,
                ProtoError::IncompatibleVersion {
                    local: CURRENT.get(),
                    remote: ancient.get()
                }
            );
        }
    }

    #[test]
    fn min_supported_never_exceeds_current() {
        assert!(MIN_SUPPORTED <= CURRENT);
    }
}
