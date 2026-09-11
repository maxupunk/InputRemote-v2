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
    // Fala com o serviço de verdade quando ele está no ar; se não estiver (não instalado, ou
    // parado), cai para o simulado, e a janela avisa o usuário disso na cara. Uma interface que
    // finge estar ligada é pior que uma que diz claramente que não está.
    let servico: Rc<dyn Servico> = match ServicoReal::conectar() {
        Ok(real) => Rc::new(real),
        Err(erro) => {
            // O motivo **não** pode ser engolido. A faixa de demonstração diz que o serviço não
            // respondeu; só esta linha diz por quê, e a diferença entre "não existe" e "permissão
            // negada" é a diferença entre dois problemas sem nada em comum.
            explicar(&erro);
            Rc::new(ServicoSimulado::new())
        }
    };
    ir_ui::janela::abrir(servico)
}

/// Escreve, no erro padrão, por que a interface não achou o serviço — e o que fazer.
fn explicar(erro: &std::io::Error) {
    let onde = ir_ui::real::endereco_do_servico();
    eprintln!("InputRemote: não consegui falar com o serviço em {onde}");
    eprintln!("  motivo: {erro}");
    match erro.kind() {
        std::io::ErrorKind::NotFound => {
            eprintln!("  o canal não existe: o serviço não está rodando.");
            if cfg!(windows) {
                eprintln!("  confira o serviço \"InputRemote\" em Serviços do Windows.");
            } else {
                eprintln!("  suba com: sudo systemctl enable --now inputremote");
            }
        }
        std::io::ErrorKind::PermissionDenied => {
            eprintln!("  o canal existe, mas este usuário não tem acesso a ele.");
            if cfg!(windows) {
                eprintln!("  reinstale a versão atual: o serviço antigo não liberava a interface.");
            } else {
                eprintln!("  entre no grupo e reinicie o computador:");
                eprintln!("    sudo usermod -aG inputremote \"$USER\"");
                eprintln!("  sair e entrar na sessão não basta no GNOME: o gerenciador da sessão");
                eprintln!(
                    "  sobrevive ao logout com os grupos antigos. Sem reiniciar, abra assim:"
                );
                eprintln!("    sg inputremote -c inputremote-ui");
            }
        }
        _ => eprintln!("  execute a interface por um terminal para ver esta mensagem inteira."),
    }
}
