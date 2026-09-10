//! O executável da interface.
//!
//! Faz uma coisa só: escolhe o serviço e abre a janela. Tudo o mais está em `ir_ui`, que é onde
//! os testes conseguem chegar.

// Sem janela de console atras da interface.
//
// Um binario do Windows nasce no subsistema "console", e o sistema abre um terminal para ele --
// que num programa de janela e uma janela preta que aparece junto, fica atras e nao serve para
// nada. Este atributo o move para o subsistema "windows".
//
// So no build de release. Em `debug` o console continua vindo, porque durante o desenvolvimento
// ele e onde panico e `eprintln!` aparecem, e perder isso custaria mais do que a janela preta
// incomoda. No release quem guarda esse tipo de coisa e o registro em arquivo (Etapa 1.4).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::rc::Rc;

use ir_ui::servico::Servico;
use ir_ui::simulado::ServicoSimulado;

/// Abre a janela.
///
/// # Errors
///
/// Repassa a falha do Slint quando não há backend gráfico disponível.
fn main() -> Result<(), slint::PlatformError> {
    // Enquanto o transporte de IPC não existir, o único serviço disponível é o simulado, e a
    // janela avisa o usuário disso na cara. A escolha acontece aqui e em lugar nenhum mais:
    // trocar por um cliente de verdade é trocar esta linha.
    let servico: Rc<dyn Servico> = Rc::new(ServicoSimulado::new());
    ir_ui::janela::abrir(servico)
}
