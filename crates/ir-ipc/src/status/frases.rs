//! As frases do estado: o resumo da tela inicial, o que impede o produto de funcionar, e a
//! situação da tela de bloqueio.
//!
//! Separadas dos dados porque mudam por motivos diferentes: os campos do [`Estado`] são o contrato
//! entre o serviço e a janela; as frases são o que a pessoa lê, e mudam a cada ajuste de texto.

use super::{Estado, LinkState, ParConhecido, Pausa};

impl Estado {
    /// A frase principal que a interface mostra, em uma linha.
    ///
    /// É a coisa mais importante da tela: se ela responder "por que não está funcionando?"
    /// sozinha, o usuário não precisa procurar em mais lugar nenhum.
    #[must_use]
    pub fn resumo(&self) -> String {
        if let Some(par) = self.par.as_ref() {
            match self.pausa {
                Some(Pausa::Aqui) => {
                    return "Pausado. Teclado, mouse e cópias não atravessam até você retomar."
                        .to_owned();
                }
                Some(Pausa::NoPar) => {
                    return format!("{} pausou o compartilhamento.", par.nome);
                }
                None => {}
            }
        }
        match (self.enlace, self.par.as_ref()) {
            (LinkState::Desconectado, None) => {
                "Nenhum computador pareado. Pareie um para começar.".to_owned()
            }
            (LinkState::Desconectado, Some(par)) => match self.ultima_queda {
                Some(motivo) => format!("{} — {}", par.nome, motivo.frase()),
                None => format!("{} está pareado, mas não está por perto.", par.nome),
            },
            (LinkState::Conectando, Some(par)) => format!("Conectando a {}…", par.nome),
            (LinkState::Conectando, None) => "Conectando…".to_owned(),
            (LinkState::Pronto, Some(par)) => self.resumo_pronto(par),
            (LinkState::Controlando, Some(par)) => format!("Controlando {}.", par.nome),
            // A frase ensina o gesto de voltar: sem ela, quem chega a um computador sendo usado de
            // longe não sabe que basta mexer no mouse (ADR-0014).
            (LinkState::Controlado, Some(par)) => format!(
                "{} está usando este computador. Mexa no mouse daqui para voltar a usá-lo.",
                par.nome
            ),
            (_, None) => self.enlace.frase().to_owned(),
        }
    }

    /// Se o teclado e o mouse daqui podem ir para o outro agora: a política deixa, e a captura está
    /// de pé.
    #[must_use]
    pub const fn vai(&self) -> bool {
        self.politica.manda() && self.captura_pronta
    }

    /// Se o outro pode vir para cá agora: a política deixa, e a injeção está de pé.
    #[must_use]
    pub const fn vem(&self) -> bool {
        self.politica.recebe() && self.agente_pronto
    }

    /// O resumo com a sessão de pé e cada um na sua tela: o que dá para fazer daqui.
    fn resumo_pronto(&self, par: &ParConhecido) -> String {
        // "Leve o ponteiro até a borda" com a captura parada mandava fazer o que não ia dar
        // certo; o porquê está no impedimento, logo abaixo (log 47).
        match (self.vai(), self.vem()) {
            (true, _) => format!("Conectado a {}. Leve o ponteiro até a borda.", par.nome),
            (false, true) => format!(
                "Conectado a {}. O teclado e o mouse de lá controlam este.",
                par.nome
            ),
            (false, false) => format!(
                "Conectado a {}, mas o teclado e o mouse não atravessam.",
                par.nome
            ),
        }
    }

    /// O que impede o produto de funcionar agora, se algo impedir.
    ///
    /// Devolve a frase de um único problema — o mais grave. Mostrar cinco avisos ao mesmo tempo
    /// é a mesma coisa que não mostrar nenhum.
    #[must_use]
    pub fn impedimento(&self) -> Option<&'static str> {
        // Qual lado falhou é o que a pessoa precisa saber: **ler** o teclado daqui é o que leva
        // ao outro; **receber** o de lá é o que deixa o outro vir. "O componente que digita" no
        // computador que só ia controlar apontava para o lugar errado (log 47).
        let sem_ler = self.politica.manda() && !self.captura_pronta;
        let sem_receber = self.politica.recebe() && !self.agente_pronto;
        match (sem_ler, sem_receber) {
            (true, true) => {
                return Some(
                    "Este computador não consegue ler o próprio teclado e mouse, nem receber os do \
                     outro. Instale a versão mais nova do InputRemote aqui; se continuar, veja o \
                     diagnóstico em Preferências.",
                );
            }
            (true, false) => {
                return Some(
                    "Este computador não consegue ler o teclado e o mouse ligados a ele, então só \
                     o outro controla este. Instale a versão mais nova do InputRemote aqui.",
                );
            }
            (false, true) => {
                return Some(
                    "Este computador não consegue receber o teclado e o mouse do outro, então só \
                     este controla o outro. Instale a versão mais nova do InputRemote aqui.",
                );
            }
            (false, false) => {}
        }
        // A tela de bloqueio não entra aqui: ela é opcional e vem desligada por padrão
        // ([04, §6](../../../docs/04-seguranca.md)). Tratá-la como impedimento deixava o
        // computador controlado sempre em laranja, com tudo funcionando — e o laranja deixa de
        // querer dizer alguma coisa. A explicação dela mora em Preferências
        // ([`Self::sobre_a_tela_de_bloqueio`]).
        None
    }

    /// A situação da digitação na tela de bloqueio, para Preferências.
    ///
    /// Não conseguir e não ter deixado são coisas diferentes, com soluções diferentes: dizer "não
    /// aceita" a quem só precisa ligar uma opção manda a pessoa investigar a instalação à toa.
    #[must_use]
    pub fn sobre_a_tela_de_bloqueio(&self) -> &'static str {
        match (
            self.nivel_privilegiado.suficiente(),
            self.bloqueio_permitido,
        ) {
            (false, _) => {
                "Este computador não consegue receber digitação na tela de bloqueio nesta \
                 instalação. Instale o InputRemote como serviço para liberar."
            }
            (true, false) => {
                "Desligada: o outro computador não digita na tela de bloqueio nem nos pedidos de \
                 permissão daqui. Ligue para poder desbloquear este computador de lá."
            }
            (true, true) => {
                "Ligada: o outro computador pode digitar a senha na tela de bloqueio e nos pedidos \
                 de permissão daqui."
            }
        }
    }
}
