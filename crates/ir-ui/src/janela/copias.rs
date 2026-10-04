//! A cópia de arquivos na janela: o cartão da cópia em curso, a velocidade dela e a lista das
//! últimas. Separado do resto da janela porque é um assunto só, e o que mais cresce.

use slint::{ComponentHandle, ModelRc, VecModel};

use super::Contexto;
use crate::copia;
use crate::gerado::Dados;

impl Contexto {
    /// Conta o que está acontecendo com uma cópia de arquivos.
    ///
    /// Na janela, num cartão que **fica** depois de terminar: a pergunta "aquilo copiou mesmo?"
    /// vem depois, quando a pessoa já está no outro computador. E, no Windows, também num aviso no
    /// canto da tela — porque quem copia está no Explorer, e não aqui.
    pub(super) fn mostrar_copia(&self, transferencia: &ir_ipc::transferencia::Transferencia) {
        let velocidade = self.medir(transferencia);
        let copia = copia::copia_ui(transferencia, velocidade);
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_copia(copia.clone());
            dados.set_tem_copia(true);
        });
        self.guardar_no_historico(transferencia);
        #[cfg(windows)]
        self.aviso
            .borrow_mut()
            .mostrar(copia, transferencia.terminou());
    }

    /// A taxa desta cópia, zerando o velocímetro quando começa outra.
    fn medir(&self, copia: &ir_ipc::transferencia::Transferencia) -> String {
        let mut velocimetro = self.velocimetro.borrow_mut();
        let mut anterior = self.copia_medida.borrow_mut();
        if anterior.as_deref() != Some(copia.nome.as_str()) {
            velocimetro.zerar();
            *anterior = Some(copia.nome.clone());
        }
        if copia.terminou() {
            // No fim não há taxa: há resultado. Mostrar a última medida ao lado de "Cópia
            // entregue" faria parecer que ainda está indo.
            velocimetro.zerar();
            return String::new();
        }
        velocimetro.medir(copia.bytes_feitos, std::time::Instant::now())
    }

    /// Guarda a cópia que terminou na lista das últimas.
    fn guardar_no_historico(&self, copia: &ir_ipc::transferencia::Transferencia) {
        if !copia.terminou() {
            return;
        }
        let mut historico = self.historico.borrow_mut();
        historico.guardar(copia);
        let itens: Vec<crate::gerado::ItemDeCopia> = historico.itens().to_vec();
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_copias_recentes(ModelRc::new(VecModel::from(itens.clone())));
        });
    }
}
