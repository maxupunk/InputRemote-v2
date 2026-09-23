//! O que a janela vê: a tradução do que o serviço sabe para o vocabulário publicado da interface.
//!
//! O serviço tira um [`Retrato`] de si mesmo — a fase da sessão, o par, o portador, o agente, as
//! permissões — e este crate o transforma no [`Estado`](ir_ipc::Estado) que a janela desenha e no
//! relatório de diagnóstico. Sem E/S e sem estado próprio além das [`Voltas`], que medem o atraso.
//!
//! Saiu do `ir-daemon` quando o serviço passou do teto de 2 500 linhas de produção com a varredura
//! de melhorias ([09, §1](../../../docs/09-padroes-de-codigo.md)). A fronteira já existia: a
//! interface não conhece o produto ([ADR-0007](../../../docs/adr/0007-ui-slint-processo-separado.md)),
//! e esta é a camada que traduz um no outro — nenhum campo daqui decide nada na sessão.

mod diagnostico;
mod medidas;
mod retrato;
mod traducao;

pub use diagnostico::{Extras, relatorio};
pub use medidas::{JANELA, Voltas, motivo_da_queda};
pub use retrato::{Entrada, ParGravado, Retrato, estado, nivel};
pub use traducao::{
    borda_de, comando_do_agente, economia_na_tela, link_state, papel_de, portador_de,
    portador_do_texto, role_de, texto_do_portador,
};
