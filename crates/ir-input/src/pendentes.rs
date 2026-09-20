//! O que foi injetado e ainda não foi solto.
//!
//! "Solta tudo" é o comando mais importante do produto: sem ele, uma queda no meio de um atalho
//! deixa o Ctrl preso na outra máquina. Mas *tudo* era literal — os dois backends soltavam toda
//! tecla e todo botão que sabiam emitir, tivessem sido pressionados ou não. No Windows isso não é
//! inócuo: um **botão direito solto** gera o menu de contexto do programa em foco, e um **Alt
//! solto** ativa a barra de menus. Era o que aparecia a cada volta do ponteiro ao servidor.
//!
//! Soltar o que não está preso não é seguro; é digitar. O injetor guarda o que apertou, e solta só
//! isso — e continua idempotente, que era a razão de soltar tudo.

use ir_proto::input::{Button, HidUsage};

/// As teclas e os botões que este injetor apertou e ainda não soltou.
#[derive(Debug, Default)]
pub(crate) struct Pendentes {
    teclas: Vec<HidUsage>,
    botoes: Vec<Button>,
}

impl Pendentes {
    /// Um registro vazio.
    pub(crate) const fn nova() -> Self {
        Self {
            teclas: Vec::new(),
            botoes: Vec::new(),
        }
    }

    /// Anota uma tecla injetada.
    pub(crate) fn tecla(&mut self, usage: HidUsage, pressionada: bool) {
        anotar(&mut self.teclas, usage, pressionada);
    }

    /// Anota um botão injetado.
    pub(crate) fn botao(&mut self, button: Button, pressionado: bool) {
        anotar(&mut self.botoes, button, pressionado);
    }

    /// O que ainda está apertado, e esquece: quem chama solta exatamente isso.
    ///
    /// Esquecer antes de o envio dar certo é de propósito. Uma segunda tentativa de soltar o que o
    /// sistema recusou repetiria a recusa; e a tecla que sobrar presa é do usuário, no teclado
    /// dele, não deste injetor.
    pub(crate) fn soltar(&mut self) -> (Vec<HidUsage>, Vec<Button>) {
        (
            std::mem::take(&mut self.teclas),
            std::mem::take(&mut self.botoes),
        )
    }
}

/// Põe na lista quando aperta, tira quando solta. Sem repetir: o teclado repete a tecla apertada.
fn anotar<T: PartialEq>(lista: &mut Vec<T>, item: T, pressionado: bool) {
    let posicao = lista.iter().position(|ja| *ja == item);
    match (pressionado, posicao) {
        (true, None) => lista.push(item),
        (false, Some(posicao)) => {
            lista.remove(posicao);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tecla(valor: u16) -> HidUsage {
        HidUsage(valor)
    }

    #[test]
    fn sem_nada_injetado_nao_ha_o_que_soltar() {
        // O defeito: aqui o Windows recebia um botão direito solto, e abria o menu de contexto do
        // programa em foco a cada volta do ponteiro.
        let mut pendentes = Pendentes::nova();
        let (teclas, botoes) = pendentes.soltar();
        assert!(teclas.is_empty() && botoes.is_empty());
    }

    #[test]
    fn solta_so_o_que_continua_apertado() {
        let mut pendentes = Pendentes::nova();
        pendentes.tecla(tecla(0xE0), true); // Ctrl
        pendentes.tecla(tecla(0x06), true); // C
        pendentes.tecla(tecla(0x06), false); // soltou o C, o Ctrl continua
        pendentes.botao(Button::Left, true);
        pendentes.botao(Button::Left, false);
        pendentes.botao(Button::Right, true);

        let (teclas, botoes) = pendentes.soltar();
        assert_eq!(teclas, vec![tecla(0xE0)]);
        assert_eq!(botoes, vec![Button::Right]);
    }

    #[test]
    fn a_repeticao_do_teclado_nao_duplica_e_soltar_duas_vezes_nao_repete() {
        let mut pendentes = Pendentes::nova();
        for _ in 0..5 {
            pendentes.tecla(tecla(0x04), true);
        }
        assert_eq!(pendentes.soltar().0, vec![tecla(0x04)]);
        assert!(pendentes.soltar().0.is_empty(), "soltar é idempotente");
    }

    #[test]
    fn uma_tecla_solta_sem_ter_sido_apertada_nao_entra() {
        let mut pendentes = Pendentes::nova();
        pendentes.tecla(tecla(0xE2), false);
        assert!(pendentes.soltar().0.is_empty());
    }
}
