//! Os recados que a interface dá fora da janela, e onde cada sistema os mostra.
//!
//! O que dizer, quando e como é de [`ir_recado`]. Aqui só se escolhe quem conta o quê:
//!
//! | | Windows | Linux |
//! |---|---|---|
//! | a cópia de arquivos | esta interface, com a barra andando | o ajudante de clipboard |
//! | o que o outro computador fez (pasta oferecida, copiar e colar mudado) | esta interface | os ajudantes |
//! | a conexão que caiu | esta interface | esta interface |
//!
//! No Linux a janela costuma estar fechada — e fechada ela não está rodando —, então quem conta o
//! que chega do serviço são os ajudantes, que vivem a sessão inteira. No Windows a interface mora
//! na bandeja e está sempre de pé.
//!
//! Se o Windows recusar a central de notificações, o retorno fica no ícone da bandeja — que gira,
//! mostra o ✓ ou o ! — e na janela. Desligadas pela pessoa, nada: ela escolheu.

use std::time::Duration;

use ir_ipc::transferencia::Transferencia;
use ir_recado::{Recado, Ritmo};

/// De quanto em quanto tempo a barra da cópia anda. Atualizar a notificação não a faz reaparecer,
/// então pode ser mais vezes que no Linux.
const INTERVALO: Duration = Duration::from_secs(1);

/// A etiqueta da notificação da cópia: a seguinte toma o lugar da anterior, em vez de empilhar.
#[cfg(windows)]
const COPIA: &str = "copia";

/// Os recados fora da janela.
#[derive(Debug)]
pub(crate) struct Recados {
    #[cfg_attr(not(windows), allow(dead_code))]
    ritmo: Ritmo,
    /// A central de notificações; `None` se o Windows a recusou.
    #[cfg(windows)]
    central: Option<ir_recado::central::Notificacoes>,
}

impl Recados {
    /// Prontos para mostrar. No Windows, registra o InputRemote na central de notificações.
    pub(crate) fn novos() -> Self {
        Self {
            ritmo: Ritmo::novo(INTERVALO),
            #[cfg(windows)]
            central: ir_recado::central::Notificacoes::abrir(),
        }
    }

    /// Uma cópia de arquivos, em qualquer fase. No Linux quem conta é o ajudante de clipboard.
    #[cfg_attr(not(windows), allow(clippy::unused_self, unused_variables))]
    pub(crate) fn copia(&mut self, copia: &Transferencia) {
        #[cfg(windows)]
        if let Some(central) = &mut self.central
            && let Some(vez) = self.ritmo.vez(copia, std::time::Instant::now())
        {
            let recado = Recado::da_copia(copia);
            // Andando, a barra anda no lugar; o primeiro recado e o fim aparecem de novo.
            let atualizou = copia.em_curso() && !vez.primeira && central.atualizar(&recado, COPIA);
            if !atualizou && !central.desligadas_pela_pessoa() {
                central.mostrar(&recado, Some(COPIA));
            }
        }
    }

    /// O que o outro computador fez e quem está aqui precisa saber: uma pasta oferecida, copiar e
    /// colar mudado. No Linux quem conta são os ajudantes.
    #[cfg_attr(not(windows), allow(clippy::unused_self, unused_variables))]
    pub(crate) fn do_par(&mut self, recado: &Recado) {
        #[cfg(windows)]
        self.avulso(recado);
    }

    /// Um recado avulso, nos dois sistemas.
    #[cfg_attr(not(windows), allow(clippy::unused_self))]
    pub(crate) fn avulso(&mut self, recado: &Recado) {
        #[cfg(windows)]
        if let Some(central) = &mut self.central
            && !central.desligadas_pela_pessoa()
        {
            central.mostrar(recado, None);
        }
        #[cfg(target_os = "linux")]
        ir_recado::linux::avisar(recado);
    }
}
