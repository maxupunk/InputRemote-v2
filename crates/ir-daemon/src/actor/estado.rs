//! O retrato do ator para a janela: o que ele sabe, entregue ao `ir-painel`, que monta o [`Estado`]
//! publicado e o diagnóstico.
//!
//! A tradução mora no `ir-painel`; aqui só se junta o que está espalhado pelo ator. Nenhum campo é
//! decorativo — cada um responde "por que não está funcionando?"
//! ([01, §5](../../../docs/01-visao-e-escopo.md)).

use ir_ipc::{Estado, Maquina, Nivel};
use ir_painel::{Entrada, Extras, ParGravado, Retrato};

use super::Daemon;
use crate::config::decode_key;

impl Daemon {
    /// O estado corrente, no vocabulário publicado da interface.
    pub(crate) fn estado(&self) -> Estado {
        let pasta = self
            .config
            .pasta_de_recebidos(&self.data_dir)
            .to_string_lossy()
            .into_owned();
        ir_painel::estado(&self.retrato(pasta))
    }

    /// O relatório de diagnóstico, uma linha por campo.
    pub(super) fn diagnostico(&self) -> String {
        let radio = match (self.radio.is_some(), self.radio_proprio) {
            (true, Some(radio)) => format!("aberto ({radio})"),
            (true, None) => "aberto".to_owned(),
            (false, _) => "indisponível".to_owned(),
        };
        let extras = Extras {
            par_versao: self
                .session
                .peer()
                .map_or_else(|| "sem sessão".to_owned(), |par| par.version.to_string()),
            rota: self.session.route_report(self.now()).to_string(),
            fase: self.session.phase().to_string(),
            alcance: self.alcance.to_string(),
            radio,
            desktops: format!("{:?}", self.desktops_do_agente),
            economia: format!(
                "aqui {:?}, no par {:?}",
                self.economia_aqui, self.economia_no_par
            ),
            pares_gravados: self.config.peers.len(),
            ajudantes: self.ajudantes.ligados(),
        };
        ir_painel::relatorio(&self.estado(), &extras)
    }

    /// Até onde esta máquina consegue receber digitação sem sessão desbloqueada.
    pub(crate) fn nivel_daqui(&self) -> Nivel {
        ir_painel::nivel(&self.entrada())
    }

    /// Guarda o nome que o par deu a si mesmo, para a tela dizer quem é mesmo com ele desligado.
    pub(crate) fn lembrar_nome_do_par(&mut self, nome: &str) {
        if self
            .config
            .peers
            .first()
            .is_none_or(|par| par.nome.as_deref() == Some(nome))
        {
            return;
        }
        // Sem esperar: chega com a sessão recém-estabelecida, e a falha só custa o nome na tela.
        if let Some(par) = self.config.peers.first_mut() {
            par.nome = Some(nome.to_owned());
        }
        self.gravador.gravar(&self.config);
    }

    /// O que esta máquina tem para digitar e capturar.
    fn entrada(&self) -> Entrada<'_> {
        Entrada {
            agente_pronto: self.agente_pronto && self.injecao_recusada.is_none(),
            injeta_direto: self.injector.is_some(),
            captura_direto: self.capturer.is_some(),
            desktops_do_agente: &self.desktops_do_agente,
        }
    }

    /// Tudo que a janela precisa, juntado do ator.
    fn retrato(&self, pasta_de_recebidos: String) -> Retrato<'_> {
        let rota_dupla = self.session.route().is_some_and(ir_session::Route::is_dual);
        Retrato {
            fase: self.session.phase(),
            politica: self.session.policy(),
            borda: self.edge,
            maquina: self.machine,
            nome: &self.nome,
            par: self.config.peers.first().map(|par| ParGravado {
                maquina: Maquina(
                    decode_key(&par.pubkey)
                        .map_or([0u8; 16], |chave| ir_transporte::maquina_da_chave(&chave).0),
                ),
                nome_gravado: par.nome.as_deref(),
                ao_vivo: self.session.peer(),
                conectado: self.linked(),
            }),
            portador: self.session.carrier(),
            portador_fixado: self.portador_fixado,
            rota_dupla,
            latencia: self.voltas.latencia(std::time::Instant::now()),
            entrada: self.entrada(),
            bloqueio_permitido: self.protegido_permitido(),
            ultima_queda: self.ultima_queda,
            recebidos_bytes: self.arquivos.recebidos().espaco(),
            economia_aqui: self.economia_aqui_na_tela(),
            economia_no_par: self.economia_no_par_na_tela(),
            pausa: self.pausa,
            par_recusa_protegido: self.par_recusa_protegido,
            pasta_de_recebidos,
            borda_travada: self.borda_travada,
            bloquear_juntos: self.config.bloquear_juntos,
        }
    }
}
