//! O quadro e os detalhes de cada segundo juntos numa [`Vista`].
//!
//! O computador manda o quadro e, logo atrás, os detalhes do mesmo segundo. Mostrar o quadro sem
//! esperar deixaria o card do Docker um segundo atrás do card do processador; esperar para sempre
//! congelaria a tela se os detalhes não viessem. Então o quadro espera **um** segundo, no máximo:
//! se o próximo quadro chega antes dos detalhes, o anterior sai com os detalhes que houver.

use ir_monitor::{Detalhes, Quadro, Vista};

/// Junta o quadro e os detalhes.
#[derive(Debug, Default)]
pub(crate) struct Montagem {
    /// Se o computador manda detalhes: só da versão 2 em diante, e depois do interesse dito.
    com_detalhes: bool,
    /// O quadro que espera os detalhes do seu segundo.
    quadro: Option<Quadro>,
    /// Os últimos detalhes.
    detalhes: Option<Detalhes>,
}

impl Montagem {
    /// Daqui em diante, o quadro espera os detalhes.
    pub(crate) fn esperar_detalhes(&mut self) {
        self.com_detalhes = true;
    }

    /// Chegou um quadro: a vista a mostrar já, se houver.
    pub(crate) fn quadro(&mut self, quadro: Quadro) -> Option<Vista> {
        if !self.com_detalhes {
            return Some(Vista::montar(&quadro, None));
        }
        let atrasado = self.quadro.replace(quadro)?;
        Some(Vista::montar(&atrasado, self.detalhes.as_ref()))
    }

    /// Chegaram os detalhes: a vista do quadro que os esperava.
    pub(crate) fn detalhes(&mut self, detalhes: Detalhes) -> Option<Vista> {
        let vista = self
            .quadro
            .take()
            .map(|quadro| Vista::montar(&quadro, Some(&detalhes)));
        self.detalhes = Some(detalhes);
        vista
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir_monitor::{Copia, FaseDaCopia, Maquina, Situacao};

    fn quadro(nome: &str) -> Quadro {
        Quadro {
            maquinas: vec![Maquina {
                nome: nome.to_owned(),
                situacao: Situacao::Aguardando,
            }],
            enlace: None,
        }
    }

    fn com_copia() -> Detalhes {
        Detalhes {
            copia: Some(Copia {
                enviando: true,
                nome: "fotos".to_owned(),
                feitos: 1,
                total: 2,
                por_segundo: None,
                fase: FaseDaCopia::Andando,
            }),
            processos: Vec::new(),
        }
    }

    #[test]
    fn sem_detalhes_o_quadro_sai_na_hora() {
        let mut montagem = Montagem::default();
        assert!(montagem.quadro(quadro("PC")).is_some());
    }

    #[test]
    fn o_quadro_espera_os_detalhes_do_seu_segundo() {
        let mut montagem = Montagem::default();
        montagem.esperar_detalhes();
        assert!(montagem.quadro(quadro("PC")).is_none());
        let vista = montagem.detalhes(com_copia()).unwrap();
        assert_eq!(vista.maquinas[0].nome, "PC");
        assert!(vista.copia.is_some());
        assert!(
            montagem.detalhes(com_copia()).is_none(),
            "sem quadro novo, nada novo a mostrar"
        );
    }

    #[test]
    fn detalhes_que_nao_vem_atrasam_um_segundo_e_nao_congelam() {
        let mut montagem = Montagem::default();
        montagem.esperar_detalhes();
        assert!(montagem.quadro(quadro("um")).is_none());
        let vista = montagem.quadro(quadro("dois")).unwrap();
        assert_eq!(vista.maquinas[0].nome, "um");
    }
}
