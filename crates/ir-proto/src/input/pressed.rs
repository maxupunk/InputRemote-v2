//! O conjunto de teclas pressionadas, limitado e ordenado.
//!
//! Este é o tipo que sustenta a meta de **zero teclas presas** de
//! `docs/01-visao-e-escopo.md` §6. Ele viaja no `StateSnapshot` a cada 250 ms e depois de
//! todo evento que possa ter perdido estado — reconexão, troca de portador, agente novo,
//! troca de desktop, retorno de suspensão.
//!
//! Duas propriedades importam mais que desempenho aqui:
//!
//! 1. **Limite duro.** Um par remoto não pode fazer o serviço `SYSTEM` alocar à vontade
//!    anunciando um milhão de teclas. O limite é [`limits::MAX_PRESSED_KEYS`] e é conferido
//!    antes de qualquer alocação.
//! 2. **Ordem canônica.** O mesmo conjunto sempre codifica os mesmos bytes, o que torna os
//!    vetores gravados estáveis e a comparação de estado trivial.

use serde::{Deserialize, Serialize};

use crate::error::{ProtoError, Result};
use crate::limits;

use super::HidUsage;

/// Conjunto ordenado e limitado de teclas pressionadas.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "Vec<HidUsage>", into = "Vec<HidUsage>")]
pub struct PressedKeys {
    /// Ordenado e sem repetição. O invariante é mantido por [`Self::insert`] e verificado
    /// na desserialização por [`Self::from_vec`].
    keys: Vec<HidUsage>,
}

impl PressedKeys {
    /// Conjunto vazio.
    #[must_use]
    pub const fn new() -> Self {
        Self { keys: Vec::new() }
    }

    /// Quantas teclas estão pressionadas.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Se nenhuma tecla está pressionada.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Se esta tecla está pressionada.
    #[must_use]
    pub fn contains(&self, usage: HidUsage) -> bool {
        self.keys.binary_search(&usage).is_ok()
    }

    /// Itera as teclas em ordem canônica.
    pub fn iter(&self) -> impl Iterator<Item = HidUsage> + '_ {
        self.keys.iter().copied()
    }

    /// Marca uma tecla como pressionada.
    ///
    /// Retorna `true` quando o conjunto mudou. Inserir uma tecla já presente não é erro:
    /// teclados repetem `KeyDown` enquanto a tecla fica apertada, e isso é normal.
    ///
    /// Quando o conjunto está cheio, a tecla é **ignorada** e a função retorna `false`.
    /// Ignorar é melhor que crescer: 32 teclas simultâneas já é além de qualquer teclado
    /// real, e crescer sem teto num processo `SYSTEM` é o que não se pode permitir.
    pub fn insert(&mut self, usage: HidUsage) -> bool {
        match self.keys.binary_search(&usage) {
            Ok(_) => false,
            Err(position) => {
                if self.keys.len() >= limits::MAX_PRESSED_KEYS {
                    return false;
                }
                self.keys.insert(position, usage);
                true
            }
        }
    }

    /// Marca uma tecla como solta.
    ///
    /// Retorna `true` quando o conjunto mudou. Soltar uma tecla que não estava pressionada
    /// não é erro — é exatamente o que acontece depois de um `ReleaseAll`, quando o
    /// `KeyUp` original chega atrasado.
    pub fn remove(&mut self, usage: HidUsage) -> bool {
        match self.keys.binary_search(&usage) {
            Ok(position) => {
                self.keys.remove(position);
                true
            }
            Err(_) => false,
        }
    }

    /// Aplica um evento de tecla.
    pub fn apply(&mut self, usage: HidUsage, pressed: bool) -> bool {
        if pressed {
            self.insert(usage)
        } else {
            self.remove(usage)
        }
    }

    /// Solta todas as teclas.
    pub fn clear(&mut self) {
        self.keys.clear();
    }

    /// As teclas que estão em `self` e não em `other`.
    ///
    /// Usado nas duas direções da reconciliação de `docs/03-protocolo.md` §7:
    /// `desejado.difference(aplicado)` é o que falta pressionar, e
    /// `aplicado.difference(desejado)` é o que falta soltar.
    pub fn difference<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = HidUsage> + 'a {
        self.keys
            .iter()
            .copied()
            .filter(move |usage| !other.contains(*usage))
    }

    /// Constrói a partir de uma lista qualquer, normalizando e validando.
    ///
    /// # Errors
    ///
    /// [`ProtoError::CountTooLarge`] quando a lista passa de [`limits::MAX_PRESSED_KEYS`].
    /// A verificação acontece **antes** de ordenar ou alocar mais nada.
    pub fn from_vec(mut keys: Vec<HidUsage>) -> Result<Self> {
        if keys.len() > limits::MAX_PRESSED_KEYS {
            return Err(ProtoError::CountTooLarge {
                what: "teclas pressionadas",
                actual: keys.len(),
                limit: limits::MAX_PRESSED_KEYS,
            });
        }
        keys.sort_unstable();
        keys.dedup();
        Ok(Self { keys })
    }
}

impl TryFrom<Vec<HidUsage>> for PressedKeys {
    type Error = ProtoError;

    fn try_from(keys: Vec<HidUsage>) -> Result<Self> {
        Self::from_vec(keys)
    }
}

impl From<PressedKeys> for Vec<HidUsage> {
    fn from(set: PressedKeys) -> Self {
        set.keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(raw: u16) -> HidUsage {
        HidUsage(raw)
    }

    #[test]
    fn insert_keeps_canonical_order() {
        let mut set = PressedKeys::new();
        for raw in [0x20u16, 0x04, 0x10, 0x08] {
            assert!(set.insert(usage(raw)));
        }
        let seen: Vec<_> = set.iter().map(HidUsage::get).collect();
        assert_eq!(seen, vec![0x04, 0x08, 0x10, 0x20]);
    }

    #[test]
    fn repeated_key_down_is_not_an_error_and_does_not_grow_the_set() {
        let mut set = PressedKeys::new();
        assert!(set.insert(usage(0x04)));
        assert!(!set.insert(usage(0x04)), "repetição não muda o conjunto");
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn releasing_an_unheld_key_is_not_an_error() {
        let mut set = PressedKeys::new();
        assert!(!set.remove(usage(0x04)));
        assert!(set.is_empty());
    }

    #[test]
    fn the_set_refuses_to_grow_past_the_limit() {
        let mut set = PressedKeys::new();
        for raw in 0..u16::try_from(limits::MAX_PRESSED_KEYS).unwrap() {
            assert!(set.insert(usage(raw + 4)));
        }
        assert_eq!(set.len(), limits::MAX_PRESSED_KEYS);
        assert!(
            !set.insert(usage(0xE7)),
            "além do limite a tecla é ignorada"
        );
        assert_eq!(
            set.len(),
            limits::MAX_PRESSED_KEYS,
            "e o conjunto não cresce"
        );
    }

    #[test]
    fn from_vec_refuses_an_oversized_announcement_before_allocating() {
        let flood = vec![usage(0x04); limits::MAX_PRESSED_KEYS + 1];
        let err = PressedKeys::from_vec(flood).unwrap_err();
        assert_eq!(
            err,
            ProtoError::CountTooLarge {
                what: "teclas pressionadas",
                actual: limits::MAX_PRESSED_KEYS + 1,
                limit: limits::MAX_PRESSED_KEYS,
            }
        );
    }

    #[test]
    fn from_vec_normalises_order_and_duplicates() {
        let set = PressedKeys::from_vec(vec![usage(9), usage(4), usage(9), usage(6)]).unwrap();
        let seen: Vec<_> = set.iter().map(HidUsage::get).collect();
        assert_eq!(seen, vec![4, 6, 9], "ordenado e sem repetição");
    }

    #[test]
    fn reconciliation_is_idempotent() {
        let desired = PressedKeys::from_vec(vec![usage(4), usage(5), usage(6)]).unwrap();
        let mut applied = PressedKeys::from_vec(vec![usage(5), usage(9)]).unwrap();

        let to_press: Vec<_> = desired.difference(&applied).collect();
        let to_release: Vec<_> = applied.difference(&desired).collect();
        assert_eq!(to_press, vec![usage(4), usage(6)]);
        assert_eq!(to_release, vec![usage(9)]);

        for key in to_press {
            applied.insert(key);
        }
        for key in to_release {
            applied.remove(key);
        }
        assert_eq!(applied, desired);

        // Reconciliar de novo não produz nenhuma ação — é a propriedade que impede
        // oscilação quando dois snapshots chegam em sequência.
        assert_eq!(desired.difference(&applied).count(), 0);
        assert_eq!(applied.difference(&desired).count(), 0);
    }

    #[test]
    fn clear_releases_everything() {
        let mut set = PressedKeys::from_vec(vec![usage(4), usage(5)]).unwrap();
        set.clear();
        assert!(set.is_empty());
    }

    #[test]
    fn apply_matches_insert_and_remove() {
        let mut set = PressedKeys::new();
        assert!(set.apply(usage(4), true));
        assert!(set.contains(usage(4)));
        assert!(set.apply(usage(4), false));
        assert!(!set.contains(usage(4)));
    }
}
