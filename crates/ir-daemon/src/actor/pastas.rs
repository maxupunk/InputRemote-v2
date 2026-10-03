//! A pasta compartilhada vista do ator: ele só repassa
//! ([ADR-0015](../../../../docs/adr/0015-pastas-compartilhadas.md)).
//!
//! Quem cuida das pastas é o ajudante, na sessão do usuário; o que vai e vem do par passa pelo
//! canal local das pastas sem tocar o ator ([`ir_canais::Pastas`]). Aqui ficam só as duas coisas que
//! são do ator: contar à faixa o que a sessão sabe do par, e levar à frente o pedido da janela.

use ir_ipc::{Falha, Pedido, Resposta};

use std::sync::atomic::{AtomicU16, Ordering};

use super::Daemon;

/// A última versão guardada em disco nesta execução, para gravar só quando mudar.
static GUARDADA: AtomicU16 = AtomicU16::new(0);

impl Daemon {
    /// Conta à faixa da pasta se o par negociou uma versão que conhece pastas, e o nome dele.
    ///
    /// Chamado a cada segundo; a faixa só muda alguma coisa quando muda de verdade.
    ///
    /// Sem sessão de entrada agora, vale o que a última disse. A versão de um par não muda sem uma
    /// negociação nova, e a sessão de entrada cai por motivos que nada têm com as pastas — o
    /// computador dormiu, o rádio reconectou. Esquecer a versão ali derrubava toda transferência de
    /// pasta em curso, com o canal de arquivos de pé (visto na prova de ponta a ponta, log 59).
    ///
    /// A versão vai para o disco quando muda, com a chave do par: na próxima subida ela já é sabida
    /// antes de a sessão de entrada voltar ([`crate::arquivos::versao_guardada`]).
    pub(super) fn informar_par_as_pastas(&self) {
        let Some(par) = self.session.peer() else {
            return;
        };
        self.arquivos
            .informar_par(Some(par.version), par.name.as_str());
        let versao = par.version.get();
        if GUARDADA.swap(versao, Ordering::Relaxed) != versao
            && let Some(pareado) = self.config.peers.first()
        {
            crate::arquivos::guardar_versao(&self.data_dir, &pareado.pubkey, par.version);
        }
    }

    /// Os pedidos da janela sobre as pastas, e o que sobrar do contrato.
    pub(super) fn tratar_pasta(
        &self,
        pedido: Pedido,
        leitor: &ir_transferencia::Leitor,
    ) -> Resposta {
        match pedido {
            Pedido::Pastas => Resposta::Pastas(
                self.pastas
                    .as_ref()
                    .map(ir_canais::Pastas::resumo)
                    .unwrap_or_default(),
            ),
            Pedido::Pasta(comando) => {
                // O ajudante do mesmo usuário, quando se sabe quem pediu.
                let quem = match leitor {
                    ir_transferencia::Leitor::Usuario { uid } => Some(format!("uid:{uid}")),
                    _ => None,
                };
                let entregue = self
                    .pastas
                    .as_ref()
                    .is_some_and(|pastas| pastas.comando(comando, quem.as_deref()));
                if entregue {
                    Resposta::Feito
                } else {
                    Resposta::Falha(Falha::SemAjudanteDasPastas)
                }
            }
            // O curinga cobre variantes futuras do contrato ainda não tratadas aqui.
            _ => Resposta::Falha(Falha::ForaDeContexto),
        }
    }
}
