//! O que a origem faz com cada mudança que a réplica manda.
//!
//! A pergunta é sempre a mesma: a réplica se baseou na versão que a origem tem agora? Se sim, a
//! mudança é aplicada. Se não, os dois lados mexeram — e a regra é a do [ADR-0015], §2: nada se
//! perde, a mais recente fica com o nome, e apagar contra editar quem vence é a edição.
//!
//! [ADR-0015]: ../../../../docs/adr/0015-pastas-compartilhadas.md

use ir_proto::message::{Entry, EntryKind, OpResult, Refusal};

use super::Origem;
use crate::acao::Acao;
use crate::conflito::{Lado, Momento, nome_de_conflito, quem_fica_com_o_nome};
use crate::ignorar::ignorar_caminho;
use crate::retrato::{Visto, dentro_de};

/// O que a origem precisa saber, além do índice, para decidir um conflito.
#[derive(Debug, Clone, Copy)]
pub struct Contexto<'a> {
    /// O nome deste computador, para a cópia de conflito da versão daqui.
    pub maquina_local: &'a str,
    /// O nome do outro, para a cópia de conflito da versão de lá.
    pub maquina_do_par: &'a str,
    /// O fuso deste computador, em segundos a leste de UTC, para a hora no nome da cópia.
    pub fuso_s: i32,
    /// Quanto o relógio da réplica está adiantado em relação ao daqui.
    pub diferenca_ns: i64,
    /// Agora, no relógio daqui.
    pub agora_ns: i64,
}

/// Um arquivo que a réplica terminou de mandar, com o conteúdo já conferido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvioRecebido {
    /// O caminho.
    pub caminho: String,
    /// A versão em que a réplica se baseou; zero para arquivo novo lá.
    pub base: u64,
    /// O resumo conferido.
    pub resumo: [u8; 32],
    /// O tamanho.
    pub tamanho: u64,
    /// A modificação, no relógio da réplica.
    pub modificado_ns: i64,
}

/// A decisão: o que responder à réplica, o que fazer no disco, e o que mudou no índice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desfecho {
    /// A resposta à réplica.
    pub resultado: OpResult,
    /// O que fazer no disco, na ordem.
    pub acoes: Vec<Acao>,
    /// As entradas que mudaram, para mandar à réplica.
    pub mudancas: Vec<Entry>,
}

impl Desfecho {
    fn recusa(motivo: Refusal) -> Self {
        Self {
            resultado: OpResult::Refused(motivo),
            acoes: Vec::new(),
            mudancas: Vec::new(),
        }
    }
}

impl Origem {
    /// Decide um arquivo que a réplica mandou.
    pub fn decidir_envio(&mut self, envio: &EnvioRecebido, contexto: &Contexto<'_>) -> Desfecho {
        if ignorar_caminho(&envio.caminho) {
            return Desfecho::recusa(Refusal::UnsafePath);
        }
        let visto = Visto::arquivo(envio.tamanho, envio.modificado_ns, Some(envio.resumo));
        let atual = self.entrada(&envio.caminho).cloned();
        match atual {
            Some(atual) if atual.kind == EntryKind::Directory => {
                Desfecho::recusa(Refusal::UnsafePath)
            }
            // Os dois lados chegaram ao mesmo conteúdo: não há o que fazer.
            Some(atual) if atual.hash == Some(envio.resumo) => Desfecho {
                resultado: OpResult::Accepted {
                    version: atual.version,
                },
                acoes: Vec::new(),
                mudancas: Vec::new(),
            },
            Some(atual) if atual.version != envio.base => {
                self.conflito_de_envio(envio, &atual, &visto, contexto)
            }
            // A base é a atual, ou o arquivo não existe — nunca existiu, ou a origem o apagou depois
            // da base da réplica: a edição vence a remoção.
            _ => {
                let mut mudancas = self.garantir_pastas(&envio.caminho, contexto.agora_ns);
                let entrada = self.gravar(&envio.caminho, &visto);
                let versao = entrada.version;
                mudancas.push(entrada);
                Desfecho {
                    resultado: OpResult::Accepted { version: versao },
                    acoes: vec![Acao::Publicar(envio.caminho.clone())],
                    mudancas,
                }
            }
        }
    }

    /// Os dois lados mudaram o mesmo arquivo: as duas versões ficam.
    fn conflito_de_envio(
        &mut self,
        envio: &EnvioRecebido,
        atual: &Entry,
        visto: &Visto,
        contexto: &Contexto<'_>,
    ) -> Desfecho {
        let vencedor = quem_fica_com_o_nome(
            atual.modified_ns,
            envio.modificado_ns,
            contexto.diferenca_ns,
        );
        let perdedora = match vencedor {
            Lado::Replica => contexto.maquina_local,
            Lado::Origem => contexto.maquina_do_par,
        };
        let quando = Momento::de_unix_ns(contexto.agora_ns, contexto.fuso_s);
        let copia = nome_de_conflito(&envio.caminho, perdedora, quando, |c| {
            self.entrada(c).is_some()
        });
        let (acoes, mudancas) = match vencedor {
            // A da réplica fica com o nome; a daqui vai para o lado, com o mesmo conteúdo.
            Lado::Replica => {
                let daqui = Visto::arquivo(atual.size, atual.modified_ns, atual.hash);
                let ao_lado = self.gravar(&copia, &daqui);
                let com_o_nome = self.gravar(&envio.caminho, visto);
                let acoes = vec![
                    Acao::Mover {
                        de: envio.caminho.clone(),
                        para: copia.clone(),
                    },
                    Acao::Publicar(envio.caminho.clone()),
                ];
                (acoes, vec![ao_lado, com_o_nome])
            }
            Lado::Origem => {
                let ao_lado = self.gravar(&copia, visto);
                (vec![Acao::Publicar(copia.clone())], vec![ao_lado])
            }
        };
        Desfecho {
            resultado: OpResult::Conflict {
                version: self.seq,
                conflict_path: copia,
            },
            acoes,
            mudancas,
        }
    }

    /// Decide uma remoção feita na réplica.
    pub fn decidir_apagar(&mut self, caminho: &str, base: u64) -> Desfecho {
        let Some(atual) = self.entrada(caminho).cloned() else {
            // Já não existe: o que a réplica queria já é verdade.
            return Desfecho {
                resultado: OpResult::Accepted { version: self.seq },
                acoes: Vec::new(),
                mudancas: Vec::new(),
            };
        };
        let mudou_depois = atual.version != base;
        // Uma subpasta com algo vivo dentro. A réplica apaga o conteúdo antes da subpasta, cada item
        // com a base dele; o que ainda está vivo aqui ela não conhecia, ou a remoção dele não valeu.
        let tem_novidade = atual.kind == EntryKind::Directory
            && self
                .entradas
                .iter()
                .any(|(c, e)| !e.deleted && dentro_de(c, caminho));
        if mudou_depois || tem_novidade {
            return Desfecho {
                resultado: OpResult::Resurrected {
                    version: atual.version,
                },
                acoes: Vec::new(),
                mudancas: Vec::new(),
            };
        }
        let mudancas = self.apagar(caminho);
        Desfecho {
            resultado: OpResult::Accepted { version: self.seq },
            acoes: vec![Acao::ParaLixeira(caminho.to_owned())],
            mudancas,
        }
    }

    /// Decide uma subpasta criada na réplica.
    pub fn decidir_criar_pasta(&mut self, caminho: &str, agora_ns: i64) -> Desfecho {
        if ignorar_caminho(caminho) {
            return Desfecho::recusa(Refusal::UnsafePath);
        }
        match self.entrada(caminho) {
            Some(e) if e.kind == EntryKind::Directory => Desfecho {
                resultado: OpResult::Accepted { version: e.version },
                acoes: Vec::new(),
                mudancas: Vec::new(),
            },
            // Um arquivo com o mesmo nome: não há como ter os dois, e o arquivo não some por isso.
            Some(_) => Desfecho::recusa(Refusal::UnsafePath),
            None => {
                let mut mudancas = self.garantir_pastas(caminho, agora_ns);
                let entrada = self.gravar(caminho, &Visto::pasta(agora_ns));
                let versao = entrada.version;
                mudancas.push(entrada);
                Desfecho {
                    resultado: OpResult::Accepted { version: versao },
                    acoes: vec![Acao::CriarPasta(caminho.to_owned())],
                    mudancas,
                }
            }
        }
    }
}
