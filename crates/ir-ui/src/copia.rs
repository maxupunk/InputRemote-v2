//! A cópia de arquivos, na forma em que a janela a desenha.
//!
//! As frases vêm de [`ir_ipc::transferencia`], que é onde elas ficam para o ajudante de clipboard
//! dizer o mesmo na notificação do sistema, no Linux. Aqui só se escolhe a cor — e "cor" aqui é
//! uma pergunta de produto: uma cópia que não atravessou é a única que pede ação de quem copiou.

use ir_ipc::transferencia::Transferencia;

use crate::gerado::CopiaUi;

/// Em curso.
pub const ANDANDO: i32 = 0;
/// Terminou e o conteúdo está lá.
pub const CONCLUIDA: i32 = 1;
/// Não aconteceu, ou parou no meio.
pub const PARADA: i32 = 2;

/// A cópia, pronta para a tela. `velocidade` vem medida de fora ([`crate::historico`]).
#[must_use]
pub fn copia_ui(copia: &Transferencia, velocidade: String) -> CopiaUi {
    CopiaUi {
        titulo: copia.titulo().into(),
        detalhe: copia.detalhe().into(),
        progresso: copia.progresso(),
        estado: estado(copia),
        velocidade: velocidade.into(),
        cancelavel: copia.sentido == ir_ipc::transferencia::Sentido::Enviando && !copia.terminou(),
        recebida: copia.sentido == ir_ipc::transferencia::Sentido::Recebendo
            && copia.terminou()
            && !copia.falhou(),
    }
}

/// O estado no vocabulário da tela.
pub(crate) fn estado(copia: &Transferencia) -> i32 {
    if copia.falhou() {
        PARADA
    } else if copia.terminou() {
        CONCLUIDA
    } else {
        ANDANDO
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use ir_ipc::transferencia::{Fase, Motivo, Sentido};

    use super::*;

    fn copia(fase: Fase) -> Transferencia {
        Transferencia {
            sentido: Sentido::Enviando,
            nome: "pasta-B".to_owned(),
            bytes_feitos: 512,
            bytes_total: 1024,
            fase,
        }
    }

    #[test]
    fn cada_fase_tem_a_sua_cor() {
        assert_eq!(estado(&copia(Fase::Andando)), ANDANDO);
        assert_eq!(estado(&copia(Fase::Anunciada)), ANDANDO);
        assert_eq!(
            estado(&copia(Fase::Concluida {
                destino: "/tmp/x".to_owned()
            })),
            CONCLUIDA
        );
        assert_eq!(estado(&copia(Fase::Parada(Motivo::CanalCaiu))), PARADA);
    }

    #[test]
    fn a_tela_recebe_o_texto_pronto() {
        let ui = copia_ui(&copia(Fase::Andando), "10,0 MB/s".to_owned());
        assert_eq!(ui.titulo, "Copiando para o outro computador");
        assert_eq!(ui.detalhe, "pasta-B · 512 B de 1,0 KB · 50%");
        assert_eq!(ui.velocidade, "10,0 MB/s");
        assert!((ui.progresso - 0.5).abs() < f32::EPSILON);
    }
}
