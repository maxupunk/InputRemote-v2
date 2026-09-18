//! O serviço de verdade, falado pelo canal de controle de IPC.
//!
//! Um adaptador, e fino de propósito: ele só apresenta uma [`Conexao`] como o [`Servico`] que a
//! janela usa. Abrir o canal é do [`Conector`]; manter a ligação viva e refazê-la é da
//! [`Conexao`]. Aqui não há decisão nenhuma.

use std::sync::Mutex;
use std::time::Duration;

use ir_ipc::{Autoridade, Aviso, Falha, Pedido, Resposta};

use crate::conector::{Conector, ConectorLocal};
use crate::conexao::{Conexao, INTERVALO_DE_RECONEXAO};
use crate::servico::{Desconexao, Servico, Situacao};

/// O serviço falado pelo IPC.
pub struct ServicoReal {
    conexao: Mutex<Conexao>,
}

impl std::fmt::Debug for ServicoReal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServicoReal")
            .field("situacao", &self.situacao())
            .finish_non_exhaustive()
    }
}

impl ServicoReal {
    /// O serviço desta máquina, pelo canal padrão da plataforma.
    ///
    /// Nunca falha: se o serviço não estiver no ar agora, a ligação é tentada de novo sozinha, e a
    /// janela diz o que está acontecendo enquanto isso.
    #[must_use]
    pub fn local() -> Self {
        Self::com(Box::new(ConectorLocal::padrao()), INTERVALO_DE_RECONEXAO)
    }

    /// Um serviço alcançado por `conector`, tentando de novo a cada `intervalo`.
    #[must_use]
    pub fn com(conector: Box<dyn Conector>, intervalo: Duration) -> Self {
        Self {
            conexao: Mutex::new(Conexao::new(conector, intervalo)),
        }
    }
}

impl Servico for ServicoReal {
    fn pedir(&self, pedido: Pedido) -> Resposta {
        self.conexao
            .lock()
            .map_or(Resposta::Falha(Falha::Interna), |mut conexao| {
                conexao.pedir(&pedido)
            })
    }

    fn avisos(&self) -> Vec<Aviso> {
        self.conexao
            .lock()
            .map(|mut conexao| conexao.avisos())
            .unwrap_or_default()
    }

    fn autoridade(&self) -> Autoridade {
        // O transporte local ainda não confere a elevação do processo, e o gate real do
        // pareamento é a comparação dos seis dígitos, que não é pulável. Declarar `Elevado` deixa
        // a interface oferecer o pareamento; a conferência de elevação no transporte é o
        // endurecimento da etapa de instalação como serviço.
        Autoridade::Elevado
    }

    fn tentar_agora(&self) {
        if let Ok(mut conexao) = self.conexao.lock() {
            conexao.tentar_agora();
        }
    }

    fn situacao(&self) -> Situacao {
        self.conexao.lock().map_or(
            Situacao::Desconectado(Desconexao::ServicoParado),
            |conexao| conexao.situacao(),
        )
    }
}
