//! Canais 1, 2 e 3 — entrada confiável, ponteiro e retorno.
//!
//! Tudo aqui está no caminho quente. Duas consequências de projeto:
//!
//! 1. Toda mensagem carrega o bitmap de modificadores, não só os eventos de modificador.
//!    Custa 1 byte e elimina a categoria de bug do modificador preso
//!    (`docs/03-protocolo.md` §5).
//! 2. Nenhuma mensagem daqui aloca. São todas `Copy`, com campos de tamanho fixo, e o teste
//!    `input_messages_stay_within_the_hot_path_budget` guarda o teto de
//!    `limits::MAX_INPUT_MESSAGE`.

use serde::{Deserialize, Serialize};

use crate::input::{Button, HidUsage, Modifiers, PointerDelta, PointerPosition, WheelDelta};
use crate::screens::Edge;

/// Canal 1 — teclas, botões e roda. Servidor → cliente, confiável.
///
/// Nada aqui pode ser perdido. Um `KeyUp` que não chega é uma tecla presa, e o produto
/// prefere derrubar o enlace a prosseguir com lacuna
/// (`docs/03-protocolo.md` §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InputMessage {
    /// Tecla pressionada.
    KeyDown {
        /// A tecla física.
        usage: HidUsage,
        /// Estado dos modificadores no instante do evento.
        mods: Modifiers,
    },
    /// Tecla solta.
    KeyUp {
        /// A tecla física.
        usage: HidUsage,
        /// Estado dos modificadores no instante do evento.
        mods: Modifiers,
    },
    /// Botão do ponteiro pressionado.
    ButtonDown {
        /// O botão.
        button: Button,
        /// Estado dos modificadores no instante do evento.
        mods: Modifiers,
    },
    /// Botão do ponteiro solto.
    ButtonUp {
        /// O botão.
        button: Button,
        /// Estado dos modificadores no instante do evento.
        mods: Modifiers,
    },
    /// Movimento de roda.
    Wheel {
        /// Deslocamento, em unidades de alta resolução.
        delta: WheelDelta,
        /// Estado dos modificadores no instante do evento.
        mods: Modifiers,
    },
    /// Solte tudo, agora.
    ///
    /// Enviada em toda falha, em toda queda e em todo encerramento. É a mensagem mais
    /// importante do protocolo: sem ela, uma sessão que termina mal deixa a máquina do outro
    /// inutilizável até reiniciar.
    ReleaseAll,
}

impl InputMessage {
    /// O estado de modificadores que esta mensagem declara, se declarar algum.
    ///
    /// [`InputMessage::ReleaseAll`] não declara: ela **é** a instrução de zerar tudo.
    #[must_use]
    pub const fn declared_modifiers(self) -> Option<Modifiers> {
        match self {
            Self::KeyDown { mods, .. }
            | Self::KeyUp { mods, .. }
            | Self::ButtonDown { mods, .. }
            | Self::ButtonUp { mods, .. }
            | Self::Wheel { mods, .. } => Some(mods),
            Self::ReleaseAll => None,
        }
    }

    /// Se esta mensagem, sozinha, deixa alguma coisa pressionada.
    ///
    /// Usado pelo núcleo para saber se ainda há estado a liberar em caso de falha.
    #[must_use]
    pub const fn presses_something(self) -> bool {
        matches!(self, Self::KeyDown { .. } | Self::ButtonDown { .. })
    }
}

/// Canal 2 — movimento do ponteiro. Servidor → cliente, o mais recente vence.
///
/// Perder uma amostra intermediária é invisível; atrasá-la não é. Por isso este canal não
/// tem confirmação nem retransmissão, e o receptor descarta o que chegar fora de ordem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PointerMessage {
    /// Movimento relativo, o caso comum.
    Motion {
        /// Deslocamento acumulado desde a última amostra enviada.
        delta: PointerDelta,
        /// Estado dos modificadores.
        mods: Modifiers,
    },
    /// Posição absoluta, para correção e para entrada de borda.
    Position {
        /// Onde o ponteiro deve estar.
        position: PointerPosition,
        /// Estado dos modificadores.
        mods: Modifiers,
    },
}

impl PointerMessage {
    /// Combina duas amostras consecutivas, quando é seguro.
    ///
    /// Dois movimentos relativos somam. Qualquer coisa envolvendo posição absoluta **não**
    /// combina: a posição mais nova já é a verdade completa, e a mais velha é descartada
    /// pelo chamador. Retornar `None` aqui é o sinal de "use só a mais nova".
    #[must_use]
    pub const fn coalesced_with(self, newer: Self) -> Option<Self> {
        match (self, newer) {
            (Self::Motion { delta: old, .. }, Self::Motion { delta: new, mods }) => {
                Some(Self::Motion {
                    delta: old.coalesced_with(new),
                    mods,
                })
            }
            _ => None,
        }
    }

    /// O estado de modificadores declarado.
    #[must_use]
    pub const fn declared_modifiers(self) -> Modifiers {
        match self {
            Self::Motion { mods, .. } | Self::Position { mods, .. } => mods,
        }
    }
}

/// Canal 3 — retorno. Cliente → servidor, confiável.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Feedback {
    /// O ponteiro encostou numa borda do cliente e o controle deve voltar.
    EdgeReached {
        /// Qual borda do cliente foi alcançada.
        edge: Edge,
        /// Onde o ponteiro estava ao alcançá-la.
        position: PointerPosition,
    },
    /// O usuário acionou o atalho de emergência no cliente.
    ///
    /// Devolve o controle imediatamente e libera tudo, mesmo com o enlace saudável.
    EmergencyRelease,
    /// O clipboard do cliente mudou e há conteúdo a oferecer.
    ClipboardChanged {
        /// Que tipo de conteúdo.
        kind: crate::message::clipboard::ClipKind,
        /// Tamanho em bytes, para o servidor decidir por qual canal pedir.
        size: u32,
    },
    /// O cliente aplicou um estado que divergia do anunciado, e corrigiu.
    ///
    /// Não é erro; é informação de diagnóstico. Muitas dessas indica perda no canal
    /// confiável, e o número aparece no relatório.
    StateReconciled {
        /// Quantas teclas precisaram ser soltas.
        released: u8,
        /// Quantas teclas precisaram ser pressionadas.
        pressed: u8,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_input_message_but_release_all_declares_modifiers() {
        let mods = Modifiers::LEFT_CTRL;
        let cases = [
            InputMessage::KeyDown {
                usage: HidUsage(0x04),
                mods,
            },
            InputMessage::KeyUp {
                usage: HidUsage(0x04),
                mods,
            },
            InputMessage::ButtonDown {
                button: Button::Left,
                mods,
            },
            InputMessage::ButtonUp {
                button: Button::Left,
                mods,
            },
            InputMessage::Wheel {
                delta: WheelDelta::up(),
                mods,
            },
        ];
        for case in cases {
            assert_eq!(case.declared_modifiers(), Some(mods), "{case:?}");
        }
        assert_eq!(InputMessage::ReleaseAll.declared_modifiers(), None);
    }

    #[test]
    fn only_down_events_press_something() {
        let mods = Modifiers::NONE;
        assert!(
            InputMessage::KeyDown {
                usage: HidUsage(0x04),
                mods
            }
            .presses_something()
        );
        assert!(
            InputMessage::ButtonDown {
                button: Button::Left,
                mods
            }
            .presses_something()
        );
        assert!(
            !InputMessage::KeyUp {
                usage: HidUsage(0x04),
                mods
            }
            .presses_something()
        );
        assert!(
            !InputMessage::ButtonUp {
                button: Button::Left,
                mods
            }
            .presses_something()
        );
        assert!(
            !InputMessage::Wheel {
                delta: WheelDelta::up(),
                mods
            }
            .presses_something()
        );
        assert!(!InputMessage::ReleaseAll.presses_something());
    }

    #[test]
    fn two_relative_motions_coalesce_into_their_sum() {
        let older = PointerMessage::Motion {
            delta: PointerDelta { dx: 3, dy: 1 },
            mods: Modifiers::NONE,
        };
        let newer = PointerMessage::Motion {
            delta: PointerDelta { dx: -1, dy: 4 },
            mods: Modifiers::LEFT_SHIFT,
        };
        let merged = older.coalesced_with(newer).unwrap();
        assert_eq!(
            merged,
            PointerMessage::Motion {
                delta: PointerDelta { dx: 2, dy: 5 },
                mods: Modifiers::LEFT_SHIFT,
            },
            "os modificadores são os da amostra mais nova"
        );
    }

    #[test]
    fn absolute_positions_never_coalesce() {
        let position = PointerMessage::Position {
            position: PointerPosition {
                monitor: crate::ids::MonitorId(0),
                x: 10,
                y: 20,
            },
            mods: Modifiers::NONE,
        };
        let motion = PointerMessage::Motion {
            delta: PointerDelta { dx: 1, dy: 1 },
            mods: Modifiers::NONE,
        };

        assert!(
            position.coalesced_with(motion).is_none(),
            "a posição já é a verdade completa"
        );
        assert!(motion.coalesced_with(position).is_none());
        assert!(position.coalesced_with(position).is_none());
    }

    #[test]
    fn coalescing_relative_motion_is_associative_in_effect() {
        let mods = Modifiers::NONE;
        let m = |dx, dy| PointerMessage::Motion {
            delta: PointerDelta { dx, dy },
            mods,
        };
        let left = m(1, 0)
            .coalesced_with(m(2, 0))
            .unwrap()
            .coalesced_with(m(3, 0))
            .unwrap();
        let right = m(1, 0)
            .coalesced_with(m(2, 0).coalesced_with(m(3, 0)).unwrap())
            .unwrap();
        assert_eq!(left, right);
        assert_eq!(left, m(6, 0));
    }
}
