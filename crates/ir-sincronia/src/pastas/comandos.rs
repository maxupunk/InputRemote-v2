//! O que a janela pede: compartilhar, criar, aceitar, recusar, parar, abrir e resolver.
//!
//! Cada pedido que não dá certo devolve uma frase para a pessoa, com o que fazer — a regra da casa
//! para toda mensagem de problema.

use std::path::{Path, PathBuf};

use ir_ipc::pastas::{ComandoDePasta, EscolhaDeConflito, IdDePasta};
use ir_pasta::{Origem, Replica};
use ir_proto::message::{DeclineFolder, FolderId, FolderMessage};

use super::Pastas;
use crate::guardado::{Guardada, Indice};
use crate::viva::Viva;
use crate::{Saida, atalho};

impl Pastas {
    /// Faz o que a janela pediu. O erro é a frase para a pessoa.
    ///
    /// # Errors
    ///
    /// A frase que diz por que não deu, e o que fazer.
    pub fn comando(
        &mut self,
        comando: ComandoDePasta,
        saida: &mut dyn Saida,
    ) -> Result<(), String> {
        match comando {
            ComandoDePasta::Compartilhar { caminho } => {
                self.compartilhar(Path::new(&caminho), saida)
            }
            ComandoDePasta::Criar { nome } => {
                let raiz = self.lugar_livre(&nome)?;
                std::fs::create_dir_all(&raiz).map_err(|e| {
                    format!("Não consegui criar a pasta ({e}). Escolha outro nome.")
                })?;
                self.compartilhar(&raiz, saida)
            }
            ComandoDePasta::Aceitar(IdDePasta(id)) => self.aceitar(FolderId(id), saida),
            ComandoDePasta::Recusar(IdDePasta(id)) => {
                let pasta = FolderId(id);
                self.ofertas.remove(&pasta);
                saida.enviar(FolderMessage::Decline {
                    folder: pasta,
                    reason: DeclineFolder::UserDeclined,
                });
                Ok(())
            }
            ComandoDePasta::Parar(IdDePasta(id)) => {
                let pasta = FolderId(id);
                saida.enviar(FolderMessage::Stop { folder: pasta });
                self.esquecer(pasta);
                Ok(())
            }
            ComandoDePasta::Abrir(IdDePasta(id)) => {
                let viva = self.vivas.get(&FolderId(id)).ok_or_else(nao_existe)?;
                abrir_no_gerenciador(viva.visivel())
            }
            ComandoDePasta::Copiado {
                pasta: IdDePasta(id),
                caminhos,
            } => {
                self.copiar_para_la(FolderId(id), caminhos, saida);
                Ok(())
            }
            ComandoDePasta::AbrirLixeira(IdDePasta(id)) => {
                let viva = self.vivas.get(&FolderId(id)).ok_or_else(nao_existe)?;
                let lixeira = viva.lixeira();
                std::fs::create_dir_all(&lixeira)
                    .map_err(|e| format!("Não consegui abrir a lixeira da pasta ({e})."))?;
                abrir_no_gerenciador(&lixeira)
            }
            ComandoDePasta::Resolver {
                pasta: IdDePasta(id),
                caminho,
                escolha,
            } => {
                let viva = self.vivas.get_mut(&FolderId(id)).ok_or_else(nao_existe)?;
                resolver(viva, &caminho, escolha)
            }
            _ => Err("Este pedido é de uma versão mais nova do InputRemote.".to_owned()),
        }
    }

    fn compartilhar(&mut self, raiz: &Path, saida: &mut dyn Saida) -> Result<(), String> {
        let raiz = std::fs::canonicalize(raiz)
            .map_err(|_| "Essa pasta não existe mais. Escolha outra.".to_owned())?;
        let raiz = sem_prefixo_estendido(&raiz);
        if !raiz.is_dir() {
            return Err("Escolha uma pasta, e não um arquivo.".to_owned());
        }
        if let Some(motivo) = self.sobreposta(&raiz) {
            return Err(motivo);
        }
        let nome = raiz
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| ir_proto::message::data::is_safe_component(n))
            .ok_or_else(|| {
                "O nome dessa pasta não pode ser usado nos dois sistemas. Renomeie e tente de novo."
                    .to_owned()
            })?
            .to_owned();
        let pasta = FolderId(rand::random());
        let guardada = Guardada {
            nome,
            raiz,
            ponto: None,
            indice: Indice::Origem(Origem::nova(pasta)),
            aceita: false,
            conflitos: Vec::new(),
        };
        let mut viva = Viva::de(guardada, &self.lugar);
        let ambiente = self.ambiente();
        viva.varrer(&ambiente, saida);
        viva.alterou();
        viva.guardar();
        if let Indice::Origem(origem) = &viva.guardada.indice
            && ambiente.conversando
        {
            let (entries, total_bytes) = origem.tamanho();
            saida.enviar(FolderMessage::Offer {
                folder: pasta,
                name: viva.guardada.nome.clone(),
                entries,
                total_bytes,
            });
        }
        self.vivas.insert(pasta, viva);
        Ok(())
    }

    fn aceitar(&mut self, pasta: FolderId, saida: &mut dyn Saida) -> Result<(), String> {
        let oferta = self.ofertas.remove(&pasta).ok_or_else(|| {
            "Essa oferta não vale mais. Peça para compartilharem de novo.".to_owned()
        })?;
        let visivel = self.lugar_livre(&oferta.nome)?;
        std::fs::create_dir_all(&visivel)
            .map_err(|e| format!("Não consegui criar a pasta ({e}). Confira o espaço em disco."))?;
        let (sob_demanda, raiz, ponto) = self.onde_guardar(pasta, &visivel);
        std::fs::create_dir_all(&raiz)
            .map_err(|e| format!("Não consegui criar a pasta ({e}). Confira o espaço em disco."))?;
        let guardada = Guardada {
            nome: oferta.nome,
            raiz,
            ponto,
            indice: Indice::Replica(Replica::nova(pasta, sob_demanda)),
            aceita: true,
            conflitos: Vec::new(),
        };
        let mut viva = Viva::de(guardada, &self.lugar);
        viva.alterou();
        viva.guardar();
        if let Some(repassar) = &self.repassar {
            viva.ligar_nuvem(&self.ambiente(), repassar);
        }
        self.vivas.insert(pasta, viva);
        atalho::por(&visivel);
        saida.enviar(FolderMessage::Accept { folder: pasta });
        if let Some(viva) = self.vivas.get_mut(&pasta) {
            viva.pedir_mudancas(saida);
        }
        Ok(())
    }

    /// Se a pasta recebida é sob demanda, e onde o conteúdo dela fica.
    ///
    /// Sob demanda só com o provedor ligado — sem ele ninguém serviria os arquivos — e onde o
    /// sistema deixa: NTFS no Windows, FUSE no Linux. No Windows o conteúdo fica na própria pasta,
    /// que é a raiz de sincronia; no Linux, num cache escondido, e a pasta visível é a montagem.
    fn onde_guardar(&self, pasta: FolderId, visivel: &Path) -> (bool, PathBuf, Option<PathBuf>) {
        let pode = self.repassar.is_some() && ir_nuvem::suportado(visivel);
        if pode && cfg!(target_os = "linux") {
            let cache = self.lugar.da_pasta(pasta).join("conteudo");
            return (true, cache, Some(visivel.to_path_buf()));
        }
        (pode, visivel.to_path_buf(), None)
    }

    /// Um lugar ainda vazio para uma pasta com este nome, na pasta padrão: o nome, ou o nome com um
    /// número.
    fn lugar_livre(&self, nome: &str) -> Result<PathBuf, String> {
        if !ir_proto::message::data::is_safe_component(nome) || nome.contains('/') {
            return Err(
                "Esse nome não pode ser usado nos dois sistemas. Use só letras, números e espaços."
                    .to_owned(),
            );
        }
        let base = &self.lugar.raiz_padrao;
        (1..1_000)
            .map(|n| {
                if n == 1 {
                    base.join(nome)
                } else {
                    base.join(format!("{nome} ({n})"))
                }
            })
            .find(|c| !c.exists())
            .ok_or_else(|| "Já há pastas demais com esse nome. Escolha outro.".to_owned())
    }

    /// Se a pasta cai dentro de outra já compartilhada, ou contém uma, ou contém o estado daqui.
    fn sobreposta(&self, raiz: &Path) -> Option<String> {
        let dentro = |a: &Path, b: &Path| a.starts_with(b);
        for viva in self.vivas.values() {
            if dentro(raiz, viva.visivel()) || dentro(viva.visivel(), raiz) {
                return Some(format!(
                    "Essa pasta está dentro de \"{}\", ou a contém, e ela já é compartilhada. \
                     Escolha uma pasta fora dela.",
                    viva.guardada.nome
                ));
            }
        }
        if dentro(&self.lugar.estado, raiz) {
            return Some("Essa pasta contém a pasta de controle do InputRemote. Escolha uma pasta mais específica, como Documentos/Projetos.".to_owned());
        }
        None
    }
}

fn nao_existe() -> String {
    "Essa pasta não é mais compartilhada.".to_owned()
}

/// Resolve um conflito no disco daqui; a sincronia leva a decisão ao outro lado.
fn resolver(viva: &mut Viva, caminho: &str, escolha: EscolhaDeConflito) -> Result<(), String> {
    let Some(posicao) = viva
        .guardada
        .conflitos
        .iter()
        .position(|(c, _)| c == caminho)
    else {
        return Err("Esse conflito já foi resolvido.".to_owned());
    };
    let (original, copia) = viva.guardada.conflitos.remove(posicao);
    // Pela montagem, no Linux: o gerenciador de arquivos vê a escolha na hora.
    let raiz = viva.pela_montagem(viva.raiz());
    let resultado = match escolha {
        EscolhaDeConflito::FicarComEsta => viva.levar_a_lixeira(&copia),
        EscolhaDeConflito::FicarComAOutra => viva.levar_a_lixeira(&original).and_then(|()| {
            let de = crate::disco::absoluto(&raiz, &copia);
            crate::disco::mover(&de, &crate::disco::absoluto(&raiz, &original))
        }),
        _ => Ok(()),
    };
    viva.alterou();
    resultado.map_err(|e| {
        format!(
            "Não consegui mexer no arquivo ({e}). Feche-o no programa que o usa e tente de novo."
        )
    })
}

/// Abre a pasta no gerenciador de arquivos do sistema.
fn abrir_no_gerenciador(raiz: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let programa = "explorer.exe";
    #[cfg(not(windows))]
    let programa = "xdg-open";
    std::process::Command::new(programa)
        .arg(raiz)
        .spawn()
        .map(drop)
        .map_err(|_| {
            format!(
                "Não consegui abrir o gerenciador de arquivos. A pasta fica em {}.",
                raiz.display()
            )
        })
}

/// O `canonicalize` do Windows devolve `\\?\C:\…`; o prefixo não serve para mostrar nem para o
/// Explorer.
fn sem_prefixo_estendido(caminho: &Path) -> PathBuf {
    let texto = caminho.to_string_lossy();
    texto
        .strip_prefix(r"\\?\")
        .filter(|resto| !resto.starts_with("UNC"))
        .map_or_else(|| caminho.to_path_buf(), PathBuf::from)
}
