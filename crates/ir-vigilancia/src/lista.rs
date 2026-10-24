//! O pareamento e a lista de painéis: o que a janela pede, e o que o monitor conta.

use ir_configuracao::{Config, Gravador, decode_key, encode_key};
use ir_ipc::{Aviso, AvisoDoPainel, ComandoDePainel, Falha, Resposta, ResumoDoPainel};
use ir_monitor_rede::{Aviso as DoMonitor, Comando};

use crate::Vigilancia;

impl Vigilancia {
    /// Um pedido da janela sobre os painéis.
    pub fn comando(
        &mut self,
        comando: ComandoDePainel,
        config: &mut Config,
        gravador: &Gravador,
    ) -> Resposta {
        let Some(monitor) = &self.monitor else {
            return Resposta::Falha(Falha::ForaDeContexto);
        };
        match comando {
            ComandoDePainel::Parear => monitor.comandar(Comando::AbrirPareamento),
            ComandoDePainel::Confirmar(conferiu) => monitor.comandar(Comando::Confirmar(conferiu)),
            ComandoDePainel::Cancelar => monitor.comandar(Comando::FecharPareamento),
            ComandoDePainel::Esquecer(id) => {
                if !config.esquecer_painel(&id) {
                    return Resposta::Falha(Falha::ForaDeContexto);
                }
                monitor.comandar(Comando::Conhecidos(config.chaves_dos_paineis()));
                gravador.gravar(config);
                self.avisar_lista(config);
            }
        }
        Resposta::Feito
    }

    /// Os painéis pareados, como a janela os mostra.
    #[must_use]
    pub fn resumo(&self, config: &Config) -> Vec<ResumoDoPainel> {
        config
            .paineis
            .iter()
            .map(|painel| ResumoDoPainel {
                id: painel.pubkey.clone(),
                nome: painel.nome.clone(),
                conectado: decode_key(&painel.pubkey)
                    .is_some_and(|chave| self.conectados.contains(&chave)),
            })
            .collect()
    }

    /// Um aviso do monitor.
    pub(crate) fn do_monitor(
        &mut self,
        aviso: DoMonitor,
        config: &mut Config,
        gravador: &Gravador,
    ) {
        match aviso {
            DoMonitor::Codigo(digitos) => self.avisar(painel(AvisoDoPainel::Codigo(digitos))),
            DoMonitor::Pareado(chave) => {
                tracing::info!(painel = %encode_key(&chave), "painel pareado");
                if config.parear_painel(&chave) {
                    gravador.gravar(config);
                }
                self.avisar(painel(AvisoDoPainel::Concluido(true)));
                self.avisar_lista(config);
            }
            DoMonitor::Apresentou { chave, nome } => {
                if config.nomear_painel(&chave, &nome) {
                    gravador.gravar(config);
                    self.avisar_lista(config);
                }
            }
            DoMonitor::PareamentoFalhou => self.avisar(painel(AvisoDoPainel::Concluido(false))),
            DoMonitor::Assinantes(quantos) => {
                self.assinantes = quantos;
                self.ajustar_vigia();
            }
            DoMonitor::Interesse(interesse) => {
                self.dos_paineis = interesse;
                self.ajustar_vigia();
            }
            DoMonitor::Conectados(chaves) => {
                self.conectados = chaves;
                self.avisar_lista(config);
            }
        }
    }

    fn avisar_lista(&self, config: &Config) {
        self.avisar(painel(AvisoDoPainel::Lista(self.resumo(config))));
    }
}

const fn painel(aviso: AvisoDoPainel) -> Aviso {
    Aviso::Painel(aviso)
}
