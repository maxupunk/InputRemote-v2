//! Baixar da origem, um arquivo por vez, em trechos.
//!
//! A réplica pede até 4 MiB de cada vez ([`ir_proto::limits::MAX_RANGE_REQUEST`]) e monta ao lado;
//! o trecho seguinte só é pedido quando o anterior chega inteiro. Assim o canal 5 nunca fica com um
//! arquivo grande inteiro na fila, e a cópia do clipboard passa entre um trecho e outro. Uma queda
//! no meio recomeça o arquivo — o trecho é a forma do pedido, e a retomada por posição fica para
//! quando a bancada mostrar que faz falta.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};

use ir_pasta::Baixar;
use ir_proto::limits::MAX_RANGE_REQUEST;
use ir_proto::message::{FolderId, FolderMessage, RangeFailure, RangeId};

use crate::Saida;

/// O que um trecho que chegou fez com o download.
#[derive(Debug)]
pub enum Chegada {
    /// Ainda falta.
    Andando,
    /// Chegou inteiro e conferido, montado em `montado`.
    Pronto {
        /// O que foi baixado.
        alvo: Baixar,
        /// Onde está.
        montado: PathBuf,
    },
    /// Não deu; o arquivo sai da fila. A próxima leva de mudanças traz o que valer.
    Desistiu(String),
}

/// A fila de downloads de uma pasta.
#[derive(Debug, Default)]
pub struct Baixas {
    fila: VecDeque<Baixar>,
    atual: Option<Baixando>,
    proximo: u32,
}

#[derive(Debug)]
struct Baixando {
    pedido: RangeId,
    alvo: Baixar,
    montado: PathBuf,
    arquivo: std::fs::File,
    recebido: u64,
    pedido_ate: u64,
    resumo: blake3::Hasher,
}

impl Baixas {
    /// Põe na fila. Uma versão mais nova do mesmo caminho substitui a que esperava.
    pub fn enfileirar(&mut self, alvo: Baixar) {
        self.fila.retain(|b| b.caminho != alvo.caminho);
        self.fila.push_back(alvo);
    }

    /// Põe na frente da fila: alguém abriu este arquivo e está esperando por ele.
    pub fn enfileirar_na_frente(&mut self, alvo: Baixar) {
        if self
            .atual
            .as_ref()
            .is_some_and(|b| b.alvo.caminho == alvo.caminho)
        {
            return;
        }
        self.fila.retain(|b| b.caminho != alvo.caminho);
        self.fila.push_front(alvo);
    }

    /// Quantos faltam, contando o que está vindo.
    #[must_use]
    pub fn pendentes(&self) -> usize {
        self.fila.len() + usize::from(self.atual.is_some())
    }

    /// Começa o próximo, se não há um vindo. Um arquivo vazio fica pronto na hora.
    ///
    /// # Errors
    ///
    /// Erro de disco ao montar.
    pub fn comecar(
        &mut self,
        raiz: &Path,
        pasta: FolderId,
        saida: &mut dyn Saida,
    ) -> std::io::Result<Option<Chegada>> {
        if self.atual.is_some() {
            return Ok(None);
        }
        let Some(alvo) = self.fila.pop_front() else {
            return Ok(None);
        };
        self.proximo = self.proximo.wrapping_add(1);
        let pedido = RangeId(self.proximo);
        let montado = crate::disco::arquivo_de_montagem(raiz, &format!("baixa-{}", pedido.0))?;
        let arquivo = std::fs::File::create(&montado)?;
        let mut baixando = Baixando {
            pedido,
            alvo,
            montado,
            arquivo,
            recebido: 0,
            pedido_ate: 0,
            resumo: blake3::Hasher::new(),
        };
        if baixando.alvo.tamanho == 0 {
            return Ok(Some(baixando.terminar()));
        }
        baixando.pedir(pasta, saida);
        self.atual = Some(baixando);
        Ok(Some(Chegada::Andando))
    }

    /// Um pedaço chegou.
    pub fn trecho(
        &mut self,
        (pedido, offset, dados): (RangeId, u64, &[u8]),
        pasta: FolderId,
        saida: &mut dyn Saida,
    ) -> Chegada {
        let Some(baixando) = self.atual.as_mut().filter(|b| b.pedido == pedido) else {
            return Chegada::Andando;
        };
        // Sobre TCP os pedaços chegam em ordem; um fora dela quer dizer que algo se perdeu.
        let fora_de_ordem = offset != baixando.recebido;
        let gravou = !fora_de_ordem && baixando.arquivo.write_all(dados).is_ok();
        if !gravou {
            return self.desistir();
        }
        baixando.resumo.update(dados);
        baixando.recebido += dados.len() as u64;
        if baixando.recebido >= baixando.alvo.tamanho {
            return self
                .atual
                .take()
                .map_or(Chegada::Desistiu(String::new()), Baixando::terminar);
        }
        if baixando.recebido >= baixando.pedido_ate {
            baixando.pedir(pasta, saida);
        }
        Chegada::Andando
    }

    /// A origem não pôde servir o trecho.
    pub fn falhou(&mut self, pedido: RangeId, motivo: RangeFailure) -> Chegada {
        if self.atual.as_ref().is_none_or(|b| b.pedido != pedido) {
            return Chegada::Andando;
        }
        if motivo == RangeFailure::Unreadable
            && let Some(baixando) = self.atual.take()
        {
            // Aberto com exclusividade lá: tenta de novo depois dos outros.
            let _ = std::fs::remove_file(&baixando.montado);
            self.fila.push_back(baixando.alvo);
            return Chegada::Andando;
        }
        self.desistir()
    }

    /// O canal caiu: o que vinha volta para o começo da fila, do zero.
    pub fn canal_caiu(&mut self) {
        if let Some(baixando) = self.atual.take() {
            let _ = std::fs::remove_file(&baixando.montado);
            self.fila.push_front(baixando.alvo);
        }
    }

    /// Esquece tudo — a pasta deixou de ser compartilhada.
    pub fn esvaziar(&mut self) {
        self.canal_caiu();
        self.fila.clear();
    }

    fn desistir(&mut self) -> Chegada {
        let Some(baixando) = self.atual.take() else {
            return Chegada::Desistiu(String::new());
        };
        let _ = std::fs::remove_file(&baixando.montado);
        Chegada::Desistiu(baixando.alvo.caminho)
    }
}

impl Baixando {
    fn pedir(&mut self, pasta: FolderId, saida: &mut dyn Saida) {
        let falta = self.alvo.tamanho - self.recebido;
        let len = u32::try_from(falta)
            .unwrap_or(u32::MAX)
            .min(MAX_RANGE_REQUEST);
        saida.enviar(FolderMessage::RequestRange {
            folder: pasta,
            request: self.pedido,
            entry: self.alvo.entrada,
            version: self.alvo.versao,
            offset: self.recebido,
            len,
        });
        self.pedido_ate = self.recebido + u64::from(len);
    }

    fn terminar(self) -> Chegada {
        let conferiu = self
            .alvo
            .resumo
            .is_none_or(|esperado| *self.resumo.finalize().as_bytes() == esperado);
        if self.arquivo.sync_all().is_err() || !conferiu {
            let _ = std::fs::remove_file(&self.montado);
            return Chegada::Desistiu(self.alvo.caminho);
        }
        drop(self.arquivo);
        Chegada::Pronto {
            alvo: self.alvo,
            montado: self.montado,
        }
    }
}
