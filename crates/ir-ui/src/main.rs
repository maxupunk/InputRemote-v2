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

use ir_ui::real::ServicoReal;
use ir_ui::servico::Servico;
use ir_ui::simulado::ServicoSimulado;

/// Abre a janela.
///
/// # Errors
///
/// Repassa a falha do Slint quando não há backend gráfico disponível.
fn main() -> Result<(), slint::PlatformError> {
    // O serviço de verdade, sempre — mesmo que ele não esteja no ar agora: a ligação é tentada de
    // novo sozinha, e a janela diz o que está acontecendo e o que fazer. O simulado só entra
    // quando pedido de propósito. Cair nele sozinho mostrava dado de mentira a quem só precisava
    // saber que o serviço estava parado — e prendia a janela ali mesmo depois de ele voltar.
    let servico: Rc<dyn Servico> = if pediu_simulado() {
        Rc::new(ServicoSimulado::new())
    } else {
        Rc::new(ServicoReal::local())
    };
    ir_ui::janela::abrir(servico)
}

/// Se a demonstração foi pedida: `--simulado` na linha de comando, ou `IR_SIMULADO` no ambiente.
fn pediu_simulado() -> bool {
    std::env::args()
        .skip(1)
        .any(|argumento| argumento == "--simulado")
        || std::env::var_os("IR_SIMULADO").is_some()
}
