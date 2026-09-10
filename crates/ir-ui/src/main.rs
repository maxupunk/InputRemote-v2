//! O executável da interface.
//!
//! Faz uma coisa só: escolhe o serviço e abre a janela. Tudo o mais está em `ir_ui`, que é onde
//! os testes conseguem chegar.

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
