//! Dois computadores simulados: um disco em memória de cada lado, o motor de verdade, e um canal
//! que se liga e desliga.
//!
//! O que corre entre os dois é exatamente o que o protocolo leva — mudanças do índice, trechos,
//! operações e desfechos —, só que entregue na hora, sem bytes no meio. A bancada não decide nada:
//! ela executa as ações que o motor pede, como o `ir-sincronia` fará com o disco de verdade.

#![allow(unreachable_pub, dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use ir_pasta::{Acao, Contexto, EnvioRecebido, Operacao, Origem, Replica, Retrato, Visto};
use ir_proto::message::{FolderId, OpResult};

/// Um resumo de 32 bytes, determinístico. Não é BLAKE3: o motor só compara resumos, nunca os
/// calcula, e a simulação não precisa de criptografia.
pub fn resumo(conteudo: &[u8]) -> [u8; 32] {
    let mut saida = [0u8; 32];
    let mut estado: u64 = 0xcbf2_9ce4_8422_2325;
    for (indice, byte) in conteudo.iter().enumerate() {
        estado ^= u64::from(*byte) ^ (indice as u64).rotate_left(17);
        estado = estado.wrapping_mul(0x0100_0000_01b3);
    }
    for (indice, slot) in saida.iter_mut().enumerate() {
        estado = estado.rotate_left(13).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ indice as u64;
        *slot = estado.to_le_bytes()[0];
    }
    saida
}

/// Um disco em memória.
#[derive(Debug, Default, Clone)]
pub struct Disco {
    pub arquivos: BTreeMap<String, (Vec<u8>, i64)>,
    pub pastas: BTreeSet<String>,
    /// Tudo o que a sincronia levou à lixeira, com o conteúdo.
    pub lixeira: Vec<(String, Vec<u8>)>,
}

impl Disco {
    pub fn escrever(&mut self, caminho: &str, conteudo: &[u8], quando: i64) {
        self.criar_acima(caminho);
        self.arquivos
            .insert(caminho.to_owned(), (conteudo.to_vec(), quando));
    }

    pub fn criar_pasta(&mut self, caminho: &str) {
        self.criar_acima(caminho);
        self.pastas.insert(caminho.to_owned());
    }

    fn criar_acima(&mut self, caminho: &str) {
        let mut acima = String::new();
        let partes: Vec<&str> = caminho.split('/').collect();
        for parte in partes.iter().take(partes.len().saturating_sub(1)) {
            if !acima.is_empty() {
                acima.push('/');
            }
            acima.push_str(parte);
            self.pastas.insert(acima.clone());
        }
    }

    /// Remove o caminho e o que está abaixo dele. Devolve o que saiu.
    pub fn remover(&mut self, caminho: &str) -> Vec<(String, Vec<u8>)> {
        let abaixo = |c: &String| c == caminho || c.starts_with(&format!("{caminho}/"));
        let saiu: Vec<(String, Vec<u8>)> = self
            .arquivos
            .iter()
            .filter(|(c, _)| abaixo(c))
            .map(|(c, (dados, _))| (c.clone(), dados.clone()))
            .collect();
        self.arquivos.retain(|c, _| !abaixo(c));
        self.pastas.retain(|c| !abaixo(c));
        saiu
    }

    pub fn para_lixeira(&mut self, caminho: &str) {
        let saiu = self.remover(caminho);
        self.lixeira.extend(saiu);
    }

    pub fn ler(&self, caminho: &str) -> Option<&Vec<u8>> {
        self.arquivos.get(caminho).map(|(dados, _)| dados)
    }

    pub fn visto(&self, caminho: &str) -> Option<Visto> {
        if self.pastas.contains(caminho) {
            return Some(Visto::pasta(0));
        }
        self.arquivos
            .get(caminho)
            .map(|(dados, quando)| Visto::arquivo(dados.len() as u64, *quando, Some(resumo(dados))))
    }

    pub fn retrato(&self) -> Retrato {
        let mut retrato = Retrato::new();
        for pasta in &self.pastas {
            retrato.insert(pasta.clone(), Visto::pasta(0));
        }
        for caminho in self.arquivos.keys() {
            if let Some(visto) = self.visto(caminho) {
                retrato.insert(caminho.clone(), visto);
            }
        }
        retrato
    }

    /// O que o usuário vê: caminhos e conteúdos, sem horários.
    pub fn arvore(&self) -> (BTreeSet<String>, BTreeMap<String, Vec<u8>>) {
        let conteudos = self
            .arquivos
            .iter()
            .map(|(c, (dados, _))| (c.clone(), dados.clone()))
            .collect();
        (self.pastas.clone(), conteudos)
    }
}

/// A origem e a réplica, cada uma com o seu disco.
pub struct Bancada {
    pub origem: Origem,
    pub disco_o: Disco,
    pub replica: Replica,
    pub disco_r: Disco,
    pub ligado: bool,
    /// O relógio comum, que avança a cada escrita de qualquer um dos lados.
    pub tempo: i64,
    /// Quantos arquivos a réplica baixou da origem.
    pub baixados: usize,
}

pub const PASTA: FolderId = FolderId([7; 16]);

impl Bancada {
    pub fn nova() -> Self {
        Self {
            origem: Origem::nova(PASTA),
            disco_o: Disco::default(),
            replica: Replica::nova(PASTA, false),
            disco_r: Disco::default(),
            ligado: true,
            tempo: 1_000,
            baixados: 0,
        }
    }

    fn agora(&mut self) -> i64 {
        self.tempo += 1;
        self.tempo
    }

    /// O usuário grava na origem.
    pub fn grava_na_origem(&mut self, caminho: &str, conteudo: &[u8]) {
        let quando = self.agora();
        self.disco_o.escrever(caminho, conteudo, quando);
    }

    /// O usuário grava na réplica.
    pub fn grava_na_replica(&mut self, caminho: &str, conteudo: &[u8]) {
        let quando = self.agora();
        self.disco_r.escrever(caminho, conteudo, quando);
    }

    /// As duas varreduras e, com o canal ligado, a conversa até não haver mais nada a dizer.
    pub fn rodar(&mut self) {
        for _ in 0..1_000 {
            self.origem.aplicar_retrato(&self.disco_o.retrato());
            self.replica.aplicar_retrato(&self.disco_r.retrato());
            if !self.ligado || !self.uma_rodada() {
                return;
            }
        }
        panic!("a sincronia não parou");
    }

    /// Uma rodada de conversa. Devolve se alguma coisa aconteceu.
    fn uma_rodada(&mut self) -> bool {
        let mut aconteceu = false;
        let mudancas = self.origem.mudancas_desde(self.replica.visto_ate());
        if !mudancas.is_empty() {
            let seq = self.origem.seq();
            if std::env::var_os("IR_TRACO").is_some() {
                let resumo: Vec<(&str, u64, bool)> = mudancas
                    .iter()
                    .map(|e| (e.path.as_str(), e.version, e.deleted))
                    .collect();
                eprintln!("  mudanças {resumo:?}");
            }
            let acoes = self.replica.aplicar_mudancas(&mudancas, seq);
            self.executar_na_replica(acoes);
            aconteceu = true;
        }
        if let Some((op, operacao)) = self.replica.proxima() {
            if std::env::var_os("IR_TRACO").is_some() {
                eprintln!("  -> {operacao:?}");
            }
            self.operar(op, operacao);
            aconteceu = true;
        }
        aconteceu
    }

    fn contexto(&self) -> Contexto<'static> {
        Contexto {
            maquina_local: "DESKTOP",
            maquina_do_par: "NOTEBOOK",
            fuso_s: 0,
            diferenca_ns: 0,
            agora_ns: 1_790_962_200_000_000_000,
        }
    }

    /// Uma operação da réplica chega à origem, e a resposta volta.
    fn operar(&mut self, op: ir_proto::message::OpId, operacao: Operacao) {
        let contexto = self.contexto();
        let (desfecho, resumo_enviado, conteudo) = match operacao {
            Operacao::Enviar { caminho, base } => {
                let Some((dados, quando)) = self.disco_r.arquivos.get(&caminho).cloned() else {
                    self.replica.desistir(op);
                    return;
                };
                let envio = EnvioRecebido {
                    caminho,
                    base,
                    resumo: resumo(&dados),
                    tamanho: dados.len() as u64,
                    modificado_ns: quando,
                };
                let desfecho = self.origem.decidir_envio(&envio, &contexto);
                (desfecho, Some(envio.resumo), Some(dados))
            }
            Operacao::Apagar { caminho, base } => {
                (self.origem.decidir_apagar(&caminho, base), None, None)
            }
            Operacao::CriarPasta { caminho } => {
                (self.origem.decidir_criar_pasta(&caminho, 0), None, None)
            }
        };
        if std::env::var_os("IR_TRACO").is_some() {
            eprintln!("  origem: {:?} -> {:?}", desfecho.resultado, desfecho.acoes);
        }
        self.executar_na_origem(&desfecho.acoes, conteudo.as_deref());
        let acoes = self
            .replica
            .resultado(op, &desfecho.resultado, resumo_enviado);
        self.executar_na_replica(acoes);
        if let OpResult::Refused(motivo) = desfecho.resultado {
            panic!("a origem recusou: {motivo:?}");
        }
    }

    fn executar_na_origem(&mut self, acoes: &[Acao], enviado: Option<&[u8]>) {
        for acao in acoes {
            let quando = self.agora();
            match acao {
                Acao::Publicar(caminho) => {
                    let dados = enviado.expect("publicar sem conteúdo");
                    self.disco_o.escrever(caminho, dados, quando);
                }
                Acao::Mover { de, para } => {
                    let saiu = self.disco_o.remover(de);
                    for (caminho, dados) in saiu {
                        let novo = caminho.replacen(de.as_str(), para, 1);
                        self.disco_o.escrever(&novo, &dados, quando);
                    }
                }
                Acao::ParaLixeira(caminho) => self.disco_o.para_lixeira(caminho),
                Acao::CriarPasta(caminho) => self.disco_o.criar_pasta(caminho),
                outra => panic!("a origem não executa {outra:?}"),
            }
            let caminho = acao.caminho().to_owned();
            if let Some(visto) = self.disco_o.visto(&caminho) {
                self.origem.acertar_com_o_disco(&caminho, &visto);
            }
        }
    }

    fn executar_na_replica(&mut self, acoes: Vec<Acao>) {
        if std::env::var_os("IR_TRACO").is_some() && !acoes.is_empty() {
            eprintln!("  réplica: {acoes:?}");
        }
        for acao in acoes {
            let quando = self.agora();
            match &acao {
                Acao::Baixar(baixar) => {
                    let Some(entrada) = self.origem.por_id(baixar.entrada).cloned() else {
                        continue;
                    };
                    if entrada.version != baixar.versao {
                        continue; // obsoleto: a mudança mais nova vem na leva seguinte
                    }
                    let dados = self
                        .disco_o
                        .ler(&entrada.path)
                        .cloned()
                        .expect("a origem tem");
                    let no_disco = self.disco_r.visto(&baixar.caminho);
                    if !self
                        .replica
                        .pode_publicar(&baixar.caminho, no_disco.as_ref())
                    {
                        continue;
                    }
                    self.disco_r.escrever(&baixar.caminho, &dados, quando);
                    let visto = self
                        .disco_r
                        .visto(&baixar.caminho)
                        .expect("acabou de gravar");
                    self.replica.baixado(&baixar.caminho, baixar.versao, visto);
                    self.baixados += 1;
                }
                Acao::Copiar { de, para } => {
                    let dados = self.disco_r.ler(de).cloned().expect("a fonte existe");
                    self.disco_r.escrever(para, &dados, quando);
                }
                Acao::Mover { de, para } => {
                    let dados = self.disco_r.remover(de);
                    let (_, dados) = dados.into_iter().next().expect("a fonte existe");
                    self.disco_r.escrever(para, &dados, quando);
                }
                Acao::CriarPasta(caminho) => self.disco_r.criar_pasta(caminho),
                Acao::ParaLixeira(caminho) => self.disco_r.para_lixeira(caminho),
                outra => panic!("a réplica não executa {outra:?}"),
            }
            let caminho = acao.caminho().to_owned();
            if let Some(visto) = self.disco_r.visto(&caminho) {
                self.replica.registrar(&caminho, visto);
            }
        }
    }

    /// As duas árvores, iguais?
    pub fn em_dia(&self) -> bool {
        self.disco_o.arvore() == self.disco_r.arvore()
    }
}
