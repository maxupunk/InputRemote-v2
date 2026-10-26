//! O painel no celular, de mentira: um aparelho já pareado e conectado, e um pareamento que chega ao
//! código alguns passos depois de pedido — como o de verdade, quando o aparelho é aberto.

use ir_ipc::{Aviso, AvisoDoPainel, ComandoDePainel, Falha, Resposta, ResumoDoPainel};

/// Os seis dígitos que o pareamento simulado mostra.
const DIGITOS: [u8; 6] = [2, 7, 5, 8, 0, 1];

/// Quantos passos o aparelho simulado leva para chegar ao código. A janela consulta a cada 200 ms.
const PASSOS_ATE_O_CODIGO: u32 = 10;

/// Os painéis da demonstração.
#[derive(Debug)]
pub(super) struct PainelSimulado {
    lista: Vec<ResumoDoPainel>,
    tique: u32,
    /// Em que passo o código aparece, com um pareamento pedido.
    codigo_em: Option<u32>,
    /// Se um aparelho novo pode pedir para parear por conta própria.
    aceita_pedidos: bool,
}

impl PainelSimulado {
    /// Um tablet já pareado e mostrando este computador.
    pub(super) fn novo() -> Self {
        Self {
            lista: vec![ResumoDoPainel {
                id: "tablet".to_owned(),
                nome: "Tablet da sala".to_owned(),
                conectado: true,
            }],
            tique: 0,
            codigo_em: None,
            aceita_pedidos: true,
        }
    }

    pub(super) fn lista(&self) -> Vec<ResumoDoPainel> {
        self.lista.clone()
    }

    pub(super) fn comando(&mut self, comando: ComandoDePainel, fila: &mut Vec<Aviso>) -> Resposta {
        match comando {
            ComandoDePainel::Parear => self.codigo_em = Some(self.tique + PASSOS_ATE_O_CODIGO),
            ComandoDePainel::Confirmar(true) => {
                self.lista.push(ResumoDoPainel {
                    id: format!("celular-{}", self.lista.len()),
                    nome: "Celular".to_owned(),
                    conectado: true,
                });
                fila.push(painel(AvisoDoPainel::Concluido(true)));
                fila.push(painel(AvisoDoPainel::Lista(self.lista())));
            }
            ComandoDePainel::Confirmar(false) | ComandoDePainel::Cancelar => {
                self.codigo_em = None;
                fila.push(painel(AvisoDoPainel::Concluido(false)));
            }
            ComandoDePainel::Esquecer(id) => {
                let antes = self.lista.len();
                self.lista.retain(|p| p.id != id);
                if self.lista.len() == antes {
                    return Resposta::Falha(Falha::ForaDeContexto);
                }
                fila.push(painel(AvisoDoPainel::Lista(self.lista())));
            }
            ComandoDePainel::AceitarPedidos(aceita) => {
                self.aceita_pedidos = aceita;
                fila.push(painel(AvisoDoPainel::AceitaPedidos(aceita)));
            }
            ComandoDePainel::ComoEstaoOsPedidos => {
                fila.push(painel(AvisoDoPainel::AceitaPedidos(self.aceita_pedidos)));
            }
        }
        Resposta::Feito
    }

    /// Um passo do relógio simulado.
    pub(super) fn avancar(&mut self, fila: &mut Vec<Aviso>) {
        self.tique = self.tique.saturating_add(1);
        if self.codigo_em.is_some_and(|quando| quando <= self.tique) {
            self.codigo_em = None;
            fila.push(painel(AvisoDoPainel::Codigo(DIGITOS)));
        }
    }
}

const fn painel(aviso: AvisoDoPainel) -> Aviso {
    Aviso::Painel(aviso)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parear_chega_ao_codigo_e_confirmar_acrescenta_o_aparelho() {
        let mut painel = PainelSimulado::novo();
        let mut fila = Vec::new();
        assert_eq!(
            painel.comando(ComandoDePainel::Parear, &mut fila),
            Resposta::Feito
        );
        for _ in 0..PASSOS_ATE_O_CODIGO {
            painel.avancar(&mut fila);
        }
        assert_eq!(fila, vec![Aviso::Painel(AvisoDoPainel::Codigo(DIGITOS))]);

        let _ = painel.comando(ComandoDePainel::Confirmar(true), &mut fila);
        assert_eq!(painel.lista().len(), 2);
    }

    #[test]
    fn esquecer_tira_da_lista_e_esquecer_de_novo_falha() {
        let mut painel = PainelSimulado::novo();
        let mut fila = Vec::new();
        let id = painel.lista()[0].id.clone();
        assert_eq!(
            painel.comando(ComandoDePainel::Esquecer(id.clone()), &mut fila),
            Resposta::Feito
        );
        assert!(painel.lista().is_empty());
        assert_eq!(
            painel.comando(ComandoDePainel::Esquecer(id), &mut fila),
            Resposta::Falha(Falha::ForaDeContexto)
        );
    }
}
