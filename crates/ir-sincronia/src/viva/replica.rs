//! A pasta do lado de quem a recebeu: aplicar o que a origem manda, baixar, e levar as mudanças
//! daqui — uma por vez, na ordem da fila.

use ir_pasta::{Acao, Baixar, Operacao};
use ir_proto::message::{FolderMessage, OpResult};
use tracing::{debug, warn};

use super::{Ambiente, Viva};
use crate::Saida;
use crate::baixa::Chegada;
use crate::envio::Envio;
use crate::guardado::Indice;

impl Viva {
    pub(super) fn na_replica(
        &mut self,
        mensagem: FolderMessage,
        ambiente: &Ambiente,
        saida: &mut dyn Saida,
    ) {
        let pasta = self.pasta();
        match mensagem {
            FolderMessage::Changes {
                entries,
                up_to,
                last,
                ..
            } => {
                let Some(replica) = self.replica() else {
                    return;
                };
                let acoes = replica.aplicar_mudancas(&entries, up_to);
                self.mudancas_por_vir = !last;
                self.agendar(acoes, ambiente);
                if last {
                    saida.enviar(FolderMessage::Acknowledge {
                        folder: pasta,
                        seq: up_to,
                    });
                }
            }
            FolderMessage::Range {
                request,
                offset,
                data,
                ..
            } => {
                // Um pedido do Windows, para um arquivo sob demanda, ou um download inteiro.
                if self.trecho_da_nuvem((request, offset, &data), saida) {
                    return;
                }
                let chegada = self.baixas.trecho((request, offset, &data), pasta, saida);
                self.chegou(chegada, ambiente);
            }
            FolderMessage::RangeFailed {
                request, reason, ..
            } => {
                if self.falha_da_nuvem(request) {
                    return;
                }
                let chegada = self.baixas.falhou(request, reason);
                self.chegou(chegada, ambiente);
            }
            FolderMessage::Credit { op, bytes, .. } => {
                let Some(envio) = self.envio.as_mut().filter(|e| e.op == op) else {
                    return;
                };
                if let Err(erro) = envio.creditar(bytes, pasta, saida) {
                    warn!(%erro, "o arquivo em envio não pôde ser lido; ele volta à fila");
                    self.desistir_do_envio();
                }
            }
            FolderMessage::Outcome { op, result, .. } => self.desfecho(op, &result, ambiente),
            _ => {}
        }
    }

    /// Pede à origem o que mudou desde a última leva aplicada.
    pub(crate) fn pedir_mudancas(&mut self, saida: &mut dyn Saida) {
        let Some(replica) = self.replica() else {
            return;
        };
        let since = replica.visto_ate();
        self.mudancas_por_vir = true;
        saida.enviar(FolderMessage::RequestChanges {
            folder: self.pasta(),
            since,
        });
    }

    /// A próxima coisa a fazer: um download, e a próxima mudança daqui.
    pub(super) fn andar_replica(&mut self, ambiente: &Ambiente, saida: &mut dyn Saida) {
        let pasta = self.pasta();
        let raiz = self.guardada.raiz.clone();
        match self.baixas.comecar(&raiz, pasta, saida) {
            Ok(Some(chegada)) => self.chegou(chegada, ambiente),
            Ok(None) => {}
            Err(erro) => warn!(%erro, "não consegui montar um download"),
        }
        if self.envio.is_some() {
            return;
        }
        let Some((op, operacao)) = self.replica().and_then(ir_pasta::Replica::proxima) else {
            return;
        };
        self.alterou();
        match operacao {
            Operacao::Enviar { caminho, base } => {
                match Envio::preparar(&raiz, (op, &caminho, base), pasta, saida) {
                    Ok(envio) => self.envio = Some(envio),
                    Err(erro) => {
                        debug!(%erro, "o arquivo não pôde ser lido agora; volta na próxima varredura");
                        if let Some(replica) = self.replica() {
                            replica.desistir(op);
                        }
                    }
                }
            }
            Operacao::Apagar { caminho, base } => saida.enviar(FolderMessage::Delete {
                folder: pasta,
                op,
                path: caminho,
                base,
            }),
            Operacao::CriarPasta { caminho } => saida.enviar(FolderMessage::CreateDirectory {
                folder: pasta,
                op,
                path: caminho,
            }),
        }
    }

    fn replica(&mut self) -> Option<&mut ir_pasta::Replica> {
        match &mut self.guardada.indice {
            Indice::Replica(replica) => Some(replica),
            Indice::Origem(_) => None,
        }
    }

    /// Executa o que é local, põe os downloads na fila e os marcadores no disco.
    fn agendar(&mut self, acoes: Vec<Acao>, ambiente: &Ambiente) {
        let acoes = self.sem_copia_sob_demanda(acoes);
        let pastas: Vec<String> = acoes
            .iter()
            .filter_map(|acao| match acao {
                Acao::CriarPasta(caminho) => Some(caminho.clone()),
                _ => None,
            })
            .collect();
        for acao in self.executar(acoes) {
            match acao {
                Acao::Baixar(alvo) => self.baixas.enfileirar(alvo),
                Acao::Marcador(alvo) => self.marcador(&alvo, ambiente),
                _ => {}
            }
        }
        for pasta in pastas {
            self.em_dia_na_nuvem(&pasta);
        }
    }

    fn desfecho(&mut self, op: ir_proto::message::OpId, resultado: &OpResult, ambiente: &Ambiente) {
        let envio = self.envio.take_if(|e| e.op == op);
        let resumo = envio.as_ref().map(|e| e.resumo);
        let caminho = envio.as_ref().map(|e| e.caminho.clone());
        if let Some(envio) = envio {
            envio.descartar();
        }
        match (resultado, caminho) {
            (OpResult::Conflict { conflict_path, .. }, Some(caminho)) => {
                self.guardada
                    .conflitos
                    .push((caminho, conflict_path.clone()));
            }
            // Foi para a origem e ficou: o ícone vira ✓.
            (OpResult::Accepted { .. }, Some(caminho)) => self.em_dia_na_nuvem(&caminho),
            _ => {}
        }
        let Some(replica) = self.replica() else {
            return;
        };
        let acoes = replica.resultado(op, resultado, resumo);
        self.agendar(acoes, ambiente);
    }

    fn desistir_do_envio(&mut self) {
        if let Some(envio) = self.envio.take() {
            let op = envio.op;
            envio.descartar();
            if let Some(replica) = self.replica() {
                replica.desistir(op);
            }
        }
    }

    /// Um download terminou: se o caminho ainda está como estava, o arquivo vai para o lugar.
    fn chegou(&mut self, chegada: Chegada, ambiente: &Ambiente) {
        let (alvo, montado) = match chegada {
            Chegada::Pronto { alvo, montado } => (alvo, montado),
            Chegada::Desistiu(caminho) => return self.download_falhou(&caminho),
            Chegada::Andando => return,
        };
        let destino = crate::disco::absoluto(&self.guardada.raiz, &alvo.caminho);
        let no_disco = crate::varredura::visto_por_fora(&destino);
        let pode = self
            .replica()
            .is_some_and(|r| r.pode_publicar(&alvo.caminho, no_disco.as_ref()));
        if !pode {
            // Mexeram no arquivo enquanto ele vinha: o daqui vale, e vai para a origem decidir.
            let _ = std::fs::remove_file(&montado);
            return self.download_falhou(&alvo.caminho);
        }
        let horario = alvo.modificado_ns.saturating_sub(ambiente.diferenca_ns);
        if let Err(erro) = crate::disco::publicar(&montado, &destino, horario) {
            warn!(%erro, "não consegui pôr no lugar um arquivo baixado");
            let _ = std::fs::remove_file(&montado);
            return;
        }
        self.publicado(&alvo, &destino);
    }

    fn publicado(&mut self, alvo: &Baixar, destino: &std::path::Path) {
        let Some(mut visto) = crate::varredura::visto_por_fora(destino) else {
            return;
        };
        visto.resumo = alvo.resumo;
        if let Some(replica) = self.replica() {
            replica.baixado(&alvo.caminho, alvo.versao, visto);
        }
        self.alterou();
        self.download_chegou(&alvo.caminho);
    }
}
