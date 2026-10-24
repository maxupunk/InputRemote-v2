//! O que o aparelho guarda: a identidade dele e os computadores pareados.
//!
//! Um arquivo JSON na pasta privada do aplicativo, que o Android não deixa outro aplicativo ler. A
//! chave secreta fica nele como no `identity.key` do serviço: quem tem o arquivo é o aparelho.

use std::path::{Path, PathBuf};

use ir_crypto::{Identity, PublicKey};
use serde::{Deserialize, Serialize};

/// O nome do arquivo, dentro da pasta do aplicativo.
const ARQUIVO: &str = "painel.json";

/// O que deu errado com o cofre.
#[derive(Debug, thiserror::Error)]
pub enum ErroDoCofre {
    /// O disco recusou.
    #[error("não foi possível ler ou gravar o cofre: {0}")]
    Disco(#[from] std::io::Error),
    /// O arquivo existe e não é um cofre.
    #[error("o cofre está corrompido: {0}")]
    Formato(#[from] serde_json::Error),
}

/// Um computador pareado com este aparelho.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Computador {
    /// O nome que ele anuncia.
    pub nome: String,
    /// A chave pública dele, em hexadecimal — a que o `IK` fixa ao voltar.
    pub chave: String,
    /// Onde o painel o achou da última vez: `ip:porta`.
    pub endereco: String,
}

impl Computador {
    /// A chave pública, se o texto gravado for uma.
    #[must_use]
    pub fn chave(&self) -> Option<PublicKey> {
        PublicKey::de_hex(&self.chave)
    }

    /// Onde o painel atende nele, pelo que foi gravado. Um IP sem porta é da porta de sempre.
    #[must_use]
    pub fn endereco_do_painel(&self) -> Option<std::net::SocketAddr> {
        self.endereco.parse().ok().or_else(|| {
            let ip = self.endereco.parse().ok()?;
            Some(crate::descoberta::do_painel(ip))
        })
    }

    /// O identificador da instalação, como a descoberta o anuncia: a chave fixa quem ele é, e o IP
    /// pode mudar.
    #[must_use]
    pub fn maquina(&self) -> Option<String> {
        self.chave()
            .map(|chave| format!("{:x}", chave.machine_id()))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Gravado {
    identidade: String,
    #[serde(default)]
    computadores: Vec<Computador>,
}

/// O cofre aberto.
pub struct Cofre {
    caminho: PathBuf,
    identidade: Identity,
    computadores: Vec<Computador>,
}

impl core::fmt::Debug for Cofre {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Cofre")
            .field("caminho", &self.caminho)
            .field("computadores", &self.computadores)
            .finish_non_exhaustive()
    }
}

impl Cofre {
    /// Abre o cofre da pasta, criando a identidade na primeira vez.
    ///
    /// # Errors
    ///
    /// [`ErroDoCofre`] se o disco recusar ou o arquivo não for um cofre.
    pub fn abrir(pasta: &Path) -> Result<Self, ErroDoCofre> {
        let caminho = pasta.join(ARQUIVO);
        let gravado: Gravado = match std::fs::read_to_string(&caminho) {
            Ok(texto) => serde_json::from_str(&texto)?,
            Err(erro) if erro.kind() == std::io::ErrorKind::NotFound => Gravado::default(),
            Err(erro) => return Err(erro.into()),
        };
        let lida = ir_crypto::bytes_de_hex(&gravado.identidade)
            .and_then(|bytes| Identity::from_secret_bytes(&bytes).ok());
        let nova = lida.is_none();
        let cofre = Self {
            caminho,
            identidade: lida.unwrap_or_else(Identity::generate),
            computadores: gravado.computadores,
        };
        if nova {
            cofre.gravar()?;
        }
        Ok(cofre)
    }

    /// A identidade deste aparelho.
    #[must_use]
    pub const fn identidade(&self) -> &Identity {
        &self.identidade
    }

    /// Os computadores pareados, o mais recente primeiro.
    #[must_use]
    pub fn computadores(&self) -> &[Computador] {
        &self.computadores
    }

    /// Guarda um computador — novo, ou com nome e endereço atualizados — no topo da lista.
    ///
    /// # Errors
    ///
    /// [`ErroDoCofre::Disco`] se não gravar.
    pub fn guardar(&mut self, computador: Computador) -> Result<(), ErroDoCofre> {
        self.computadores.retain(|c| c.chave != computador.chave);
        self.computadores.insert(0, computador);
        self.gravar()
    }

    /// Esquece o computador desta chave.
    ///
    /// # Errors
    ///
    /// [`ErroDoCofre::Disco`] se não gravar.
    pub fn esquecer(&mut self, chave: &str) -> Result<(), ErroDoCofre> {
        self.computadores.retain(|c| c.chave != chave);
        self.gravar()
    }

    /// Grava num arquivo ao lado e troca de uma vez: um aparelho que desliga no meio não perde a
    /// identidade.
    fn gravar(&self) -> Result<(), ErroDoCofre> {
        let gravado = Gravado {
            identidade: ir_proto::ids::Hex(self.identidade.secret_bytes().as_bytes()).to_string(),
            computadores: self.computadores.clone(),
        };
        let texto = serde_json::to_string_pretty(&gravado)?;
        let temporario = self.caminho.with_extension("json.novo");
        std::fs::write(&temporario, texto)?;
        std::fs::rename(&temporario, &self.caminho)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("ir-cofre-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pasta);
        std::fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    #[test]
    fn a_identidade_nasce_uma_vez_e_volta_igual() {
        let dir = pasta("identidade");
        let primeira = Cofre::abrir(&dir).unwrap().identidade().public();
        let segunda = Cofre::abrir(&dir).unwrap().identidade().public();
        assert_eq!(primeira, segunda);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn guardar_poe_no_topo_sem_repetir_e_esquecer_tira() {
        let dir = pasta("lista");
        let mut cofre = Cofre::abrir(&dir).unwrap();
        let chave = Identity::generate().public().hex();
        let pc = |nome: &str, endereco: &str| Computador {
            nome: nome.to_owned(),
            chave: chave.clone(),
            endereco: endereco.to_owned(),
        };
        cofre.guardar(pc("DESKTOP", "10.0.0.2")).unwrap();
        cofre.guardar(pc("DESKTOP", "10.0.0.9")).unwrap();
        let reaberto = Cofre::abrir(&dir).unwrap();
        assert_eq!(reaberto.computadores(), [pc("DESKTOP", "10.0.0.9")]);
        assert!(reaberto.computadores()[0].maquina().is_some());

        cofre.esquecer(&chave).unwrap();
        assert!(Cofre::abrir(&dir).unwrap().computadores().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
