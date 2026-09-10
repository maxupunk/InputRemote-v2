//! As fases da sessão, e quais transições existem.
//!
//! Quatro fases, e a mesma máquina serve aos dois papéis — o que muda é o significado de
//! [`Phase::Engaged`]: no servidor é "o controle está no par", no cliente é "estou recebendo
//! entrada". Uma máquina por papel duplicaria as regras de queda e de liberação de teclas,
//! que são as que não podem divergir.
//!
//! As transições são enumeradas e testadas. Uma transição não listada não acontece, e
//! [`Phase::can_move_to`] é o que diz isso — não um comentário.

/// Em que ponto a sessão está.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Phase {
    /// Nenhum portador disponível. Nada acontece.
    #[default]
    Offline,

    /// Um portador subiu; o handshake está em andamento.
    ///
    /// Ainda não se injeta nem se captura nada: as duas pontas não concordaram na versão nem
    /// trocaram capacidades.
    Handshaking,

    /// A sessão está estabelecida e o controle está **desta** máquina.
    ///
    /// No servidor: o ponteiro está na tela local, e a borda do par é vigiada.
    /// No cliente: nada a fazer além de responder ao *heartbeat*.
    Ready,

    /// O controle está do outro lado.
    ///
    /// No servidor: entrada local suprimida, eventos indo para o par.
    /// No cliente: injetando o que chega.
    Engaged,
}

impl Phase {
    /// Todas as fases, para varreduras e testes exaustivos.
    pub const ALL: [Self; 4] = [Self::Offline, Self::Handshaking, Self::Ready, Self::Engaged];

    /// As transições que existem, como tabela.
    ///
    /// Escrita como dado e não como `match` de propósito: a especificação de
    /// `docs/02-arquitetura.md` é uma tabela, e o código que a implementa deve ser uma
    /// tabela também. Assim não há como um braço de `match` divergir da intenção sem que a
    /// linha correspondente mude.
    ///
    /// Note as duas ausências deliberadas: não se vai de [`Phase::Offline`] direto para
    /// [`Phase::Ready`] (sem handshake não há sessão), e não se vai de [`Phase::Handshaking`]
    /// direto para [`Phase::Engaged`] (não se entrega o controle a um par que ainda não
    /// confirmou quem é).
    const TRANSITIONS: [(Self, Self); 10] = [
        // Cair para offline é sempre possível: é o que toda falha faz.
        (Self::Offline, Self::Offline),
        (Self::Handshaking, Self::Offline),
        (Self::Ready, Self::Offline),
        (Self::Engaged, Self::Offline),
        // O caminho de subida.
        (Self::Offline, Self::Handshaking),
        (Self::Handshaking, Self::Ready),
        // O controle indo e voltando.
        (Self::Ready, Self::Engaged),
        (Self::Engaged, Self::Ready),
        // Reconectar sem perder a sessão passa pelo handshake de novo.
        (Self::Ready, Self::Handshaking),
        (Self::Engaged, Self::Handshaking),
    ];

    /// Se existe transição direta desta fase para aquela.
    #[must_use]
    pub fn can_move_to(self, next: Self) -> bool {
        Self::TRANSITIONS.contains(&(self, next))
    }

    /// Se a sessão está estabelecida — handshake concluído e enlace vivo.
    #[must_use]
    pub const fn is_established(self) -> bool {
        matches!(self, Self::Ready | Self::Engaged)
    }

    /// Se há alguma coisa possivelmente pressionada do outro lado.
    ///
    /// É a pergunta que decide se um `ReleaseAll` precisa ser emitido ao cair. Emitir a mais
    /// é inofensivo — `ReleaseAll` é idempotente —, e emitir a menos deixa tecla presa.
    #[must_use]
    pub const fn may_hold_input(self) -> bool {
        matches!(self, Self::Engaged)
    }

    /// Nome estável, para interface e log.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Offline => "desconectado",
            Self::Handshaking => "conectando",
            Self::Ready => "pronto",
            Self::Engaged => "em uso",
        }
    }
}

impl core::fmt::Display for Phase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_no_repeated_entry() {
        for (index, pair) in Phase::TRANSITIONS.iter().enumerate() {
            assert!(
                !Phase::TRANSITIONS
                    .iter()
                    .skip(index + 1)
                    .any(|other| other == pair),
                "{pair:?} aparece duas vezes na tabela"
            );
        }
    }

    #[test]
    fn the_table_covers_exactly_ten_transitions() {
        // O número é parte da especificação: se ele mudar, alguém acrescentou ou removeu um
        // caminho, e isso precisa ser deliberado.
        assert_eq!(Phase::TRANSITIONS.len(), 10);
    }

    #[test]
    fn offline_only_leads_to_itself_or_to_the_handshake() {
        for target in Phase::ALL {
            let allowed = matches!(target, Phase::Offline | Phase::Handshaking);
            assert_eq!(
                Phase::Offline.can_move_to(target),
                allowed,
                "offline → {target}"
            );
        }
    }

    #[test]
    fn there_is_no_shortcut_from_offline_to_a_working_session() {
        assert!(
            !Phase::Offline.can_move_to(Phase::Ready),
            "sem handshake não há sessão"
        );
        assert!(!Phase::Offline.can_move_to(Phase::Engaged));
    }

    #[test]
    fn control_is_never_handed_over_before_the_handshake_finishes() {
        assert!(!Phase::Handshaking.can_move_to(Phase::Engaged));
    }

    #[test]
    fn every_phase_can_fall_to_offline() {
        for phase in Phase::ALL {
            assert!(
                phase.can_move_to(Phase::Offline),
                "{phase} precisa poder cair"
            );
        }
    }

    #[test]
    fn only_engaged_may_be_holding_input() {
        for phase in Phase::ALL {
            assert_eq!(phase.may_hold_input(), phase == Phase::Engaged, "{phase}");
        }
    }

    #[test]
    fn established_means_ready_or_engaged() {
        assert!(Phase::Ready.is_established());
        assert!(Phase::Engaged.is_established());
        assert!(!Phase::Offline.is_established());
        assert!(
            !Phase::Handshaking.is_established(),
            "handshake ainda não é sessão"
        );
    }

    #[test]
    fn every_phase_has_a_name_and_they_are_distinct() {
        let names: Vec<&str> = Phase::ALL.iter().map(|p| p.name()).collect();
        for (index, name) in names.iter().enumerate() {
            assert!(!name.is_empty());
            assert!(
                !names.iter().skip(index + 1).any(|other| other == name),
                "nome repetido"
            );
        }
    }

    #[test]
    fn the_default_phase_is_the_safe_one() {
        assert_eq!(
            Phase::default(),
            Phase::Offline,
            "começar conectado seria mentira"
        );
    }
}
