//! A pasta do lado de quem a compartilhou: responder à réplica, servir trechos, receber o que ela
//! manda e decidir — com o motor — o que fica.

use std::io::{Read, Seek, SeekFrom};

use ir_pasta::{Acao, Contexto, Desfecho, EnvioRecebido};
use ir_proto::limits::MAX_FILE_BLOCK;
use ir_proto::message::{
    EntryId, FolderMessage, OpId, OpResult, RangeFailure, RangeId, Refusal, changes_messages,
};
use tracing::warn;

use super::{Ambiente, Viva};
use crate::Saida;
use crate::envio::Recebimento;
use crate::guardado::Indice;

impl Viva {
    pub(super) fn na_origem(
        &mut self,
        mensagem: FolderMessage,
        ambiente: &Ambiente,
        saida: &mut dyn Saida,
    ) {
        match mensagem {
            FolderMessage::Accept { .. } => {
                self.guardada.aceita = true;
                self.alterou();
            }
            FolderMessage::RequestChanges { since, .. } => {
                if !self.guardada.aceita {
                    self.guardada.aceita = true;
                    self.alterou();
                }
                self.enviado_ate = Some(since);
                self.empurrar(ambiente, saida);
            }
            FolderMessage::RequestRange {
                request,
                entry,
                version,
                offset,
                len,
                ..
            } => self.servir_trecho(request, (entry, version), (offset, len), saida),
            envio @ (FolderMessage::Upload { .. }
            | FolderMessage::UploadBlock { .. }
            | FolderMessage::UploadEnd { .. }) => self.no_envio(envio, ambiente, saida),
            FolderMessage::Delete { op, path, base, .. } => {
                let desfecho = self.origem().map(|o| o.decidir_apagar(&path, base));
                self.responder((op, desfecho), None, ambiente, saida);
            }
            FolderMessage::CreateDirectory { op, path, .. } => {
                let agora = crate::disco::nanos_agora();
                let desfecho = self.origem().map(|o| o.decidir_criar_pasta(&path, agora));
                self.responder((op, desfecho), None, ambiente, saida);
            }
            // A réplica desta versão não renomeia: manda remoção e criação. Quem renomear sem a
            // origem saber é recusado, e manda de novo do outro jeito.
            FolderMessage::Rename { op, .. } => self.recusar(op, Refusal::UnsafePath, saida),
            _ => {}
        }
    }

    /// As três mensagens de um arquivo vindo da réplica: o anúncio, os blocos e o fim.
    fn no_envio(&mut self, mensagem: FolderMessage, ambiente: &Ambiente, saida: &mut dyn Saida) {
        let pasta = self.pasta();
        match mensagem {
            FolderMessage::Upload {
                op,
                path,
                base,
                size,
                hash,
                modified_ns,
                ..
            } => {
                let anuncio = EnvioRecebido {
                    caminho: path,
                    base,
                    resumo: hash,
                    tamanho: size,
                    modificado_ns: modified_ns,
                };
                self.abrir_recebimento(op, anuncio, ambiente, saida);
            }
            FolderMessage::UploadBlock {
                op, offset, data, ..
            } => {
                let Some(recebimento) = self.recebendo.get_mut(&op) else {
                    return;
                };
                if recebimento
                    .bloco((op, offset, &data), pasta, saida)
                    .is_err()
                {
                    self.recusar(op, Refusal::HashMismatch, saida);
                }
            }
            FolderMessage::UploadEnd { op, .. } => self.concluir_envio(op, ambiente, saida),
            _ => {}
        }
    }

    /// Manda à réplica o que ela ainda não viu, se ela já pediu nesta conexão.
    pub(super) fn empurrar(&mut self, ambiente: &Ambiente, saida: &mut dyn Saida) {
        if !ambiente.conversando || !self.guardada.aceita {
            return;
        }
        let Indice::Origem(origem) = &self.guardada.indice else {
            return;
        };
        let Some(ate) = self.enviado_ate.filter(|ate| *ate < origem.seq()) else {
            return;
        };
        let seq = origem.seq();
        for mensagem in changes_messages(self.pasta(), origem.mudancas_desde(ate), seq) {
            saida.enviar(mensagem);
        }
        self.enviado_ate = Some(seq);
    }

    fn origem(&mut self) -> Option<&mut ir_pasta::Origem> {
        match &mut self.guardada.indice {
            Indice::Origem(origem) => Some(origem),
            Indice::Replica(_) => None,
        }
    }

    fn servir_trecho(
        &self,
        pedido: RangeId,
        (entrada, versao): (EntryId, u64),
        (offset, len): (u64, u32),
        saida: &mut dyn Saida,
    ) {
        let pasta = self.pasta();
        let Indice::Origem(origem) = &self.guardada.indice else {
            return;
        };
        let recusa = |motivo| FolderMessage::RangeFailed {
            folder: pasta,
            request: pedido,
            reason: motivo,
        };
        let Some(atual) = origem.por_id(entrada) else {
            saida.enviar(recusa(RangeFailure::Missing));
            return;
        };
        if atual.version != versao {
            saida.enviar(recusa(RangeFailure::Stale));
            return;
        }
        let caminho = crate::disco::absoluto(&self.guardada.raiz, &atual.path);
        let aberto = std::fs::File::open(&caminho).and_then(|mut arquivo| {
            arquivo.seek(SeekFrom::Start(offset))?;
            Ok(arquivo)
        });
        let mut arquivo = match aberto {
            Ok(arquivo) => arquivo,
            Err(erro) if erro.kind() == std::io::ErrorKind::NotFound => {
                saida.enviar(recusa(RangeFailure::Missing));
                return;
            }
            Err(_) => {
                saida.enviar(recusa(RangeFailure::Unreadable));
                return;
            }
        };
        let mut falta = u64::from(len);
        let mut posicao = offset;
        let mut bloco = vec![0u8; MAX_FILE_BLOCK];
        while falta > 0 {
            let cabe = usize::try_from(falta)
                .unwrap_or(MAX_FILE_BLOCK)
                .min(MAX_FILE_BLOCK);
            let lidos = match arquivo.read(bloco.get_mut(..cabe).unwrap_or_default()) {
                Ok(0) => break,
                Ok(lidos) => lidos,
                Err(_) => {
                    saida.enviar(recusa(RangeFailure::Unreadable));
                    return;
                }
            };
            saida.enviar(FolderMessage::Range {
                folder: pasta,
                request: pedido,
                offset: posicao,
                data: bloco.get(..lidos).unwrap_or_default().to_vec(),
            });
            posicao += lidos as u64;
            falta -= lidos as u64;
        }
    }

    fn abrir_recebimento(
        &mut self,
        op: OpId,
        anuncio: EnvioRecebido,
        ambiente: &Ambiente,
        saida: &mut dyn Saida,
    ) {
        let pasta = self.pasta();
        let raiz = self.guardada.raiz.clone();
        // O conteúdo já está aqui — um arquivo renomeado, ou copiado dentro da pasta: copia daqui.
        let ja_tem = self
            .origem()
            .and_then(|o| o.com_resumo(&anuncio.resumo).map(str::to_owned));
        if let Some(fonte) = ja_tem {
            let copia = crate::disco::arquivo_de_montagem(&raiz, &format!("recebe-{}", op.0))
                .and_then(|montado| {
                    std::fs::copy(crate::disco::absoluto(&raiz, &fonte), &montado)?;
                    Ok(montado)
                });
            if let Ok(montado) = copia {
                saida.enviar(FolderMessage::AlreadyHave { folder: pasta, op });
                self.decidir_envio((op, &anuncio), &montado, ambiente, saida);
                return;
            }
        }
        match Recebimento::abrir(&raiz, (op, anuncio), pasta, saida) {
            Ok(recebimento) => {
                let vazio = recebimento.anuncio.tamanho == 0;
                self.recebendo.insert(op, recebimento);
                if vazio {
                    self.concluir_envio(op, ambiente, saida);
                }
            }
            Err(erro) => {
                warn!(%erro, "não consegui montar o arquivo que chega da réplica");
                self.recusar(op, Refusal::NoDiskSpace, saida);
            }
        }
    }

    fn concluir_envio(&mut self, op: OpId, ambiente: &Ambiente, saida: &mut dyn Saida) {
        let Some(mut recebimento) = self.recebendo.remove(&op) else {
            return;
        };
        if !recebimento.conferir() {
            let _ = std::fs::remove_file(&recebimento.montado);
            self.recusar(op, Refusal::HashMismatch, saida);
            return;
        }
        let montado = recebimento.montado.clone();
        self.decidir_envio((op, &recebimento.anuncio), &montado, ambiente, saida);
    }

    fn decidir_envio(
        &mut self,
        (op, anuncio): (OpId, &EnvioRecebido),
        montado: &std::path::Path,
        ambiente: &Ambiente,
        saida: &mut dyn Saida,
    ) {
        let lugar = &ambiente.lugar;
        let contexto = Contexto {
            maquina_local: &lugar.maquina,
            maquina_do_par: &ambiente.nome_do_par,
            fuso_s: lugar.fuso_s,
            diferenca_ns: ambiente.diferenca_ns,
            agora_ns: crate::disco::nanos_agora(),
        };
        let desfecho = self.origem().map(|o| o.decidir_envio(anuncio, &contexto));
        if let Some(OpResult::Conflict { conflict_path, .. }) =
            desfecho.as_ref().map(|d| &d.resultado)
        {
            let par = (anuncio.caminho.clone(), conflict_path.clone());
            self.guardada.conflitos.push(par);
        }
        // O horário do arquivo publicado é o da réplica, trazido para o relógio daqui.
        let horario = anuncio.modificado_ns.saturating_sub(ambiente.diferenca_ns);
        self.responder((op, desfecho), Some((montado, horario)), ambiente, saida);
        let _ = std::fs::remove_file(montado);
    }

    /// Executa um desfecho, conta à réplica e empurra as mudanças.
    fn responder(
        &mut self,
        (op, desfecho): (OpId, Option<Desfecho>),
        montado: Option<(&std::path::Path, i64)>,
        ambiente: &Ambiente,
        saida: &mut dyn Saida,
    ) {
        let Some(desfecho) = desfecho else {
            return;
        };
        for acao in &desfecho.acoes {
            if let (Acao::Publicar(caminho), Some((de, horario))) = (acao, montado) {
                let destino = crate::disco::absoluto(&self.guardada.raiz, caminho);
                if let Err(erro) = crate::disco::publicar(de, &destino, horario) {
                    warn!(%erro, "não consegui publicar o arquivo que veio da réplica");
                }
                self.registrar(caminho);
            } else {
                self.executar(vec![acao.clone()]);
            }
        }
        self.alterou();
        saida.enviar(FolderMessage::Outcome {
            folder: self.pasta(),
            op,
            result: desfecho.resultado,
        });
        self.empurrar(ambiente, saida);
    }

    fn recusar(&mut self, op: OpId, motivo: Refusal, saida: &mut dyn Saida) {
        if let Some(recebimento) = self.recebendo.remove(&op) {
            let _ = std::fs::remove_file(&recebimento.montado);
        }
        saida.enviar(FolderMessage::Outcome {
            folder: self.pasta(),
            op,
            result: OpResult::Refused(motivo),
        });
    }
}
