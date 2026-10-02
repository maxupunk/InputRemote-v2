//! O ajudante que sobreviveu a uma atualização sai sozinho.
//!
//! O ajudante de clipboard roda na sessão do usuário, e atualizar o pacote troca o executável em
//! disco sem encerrar o processo. Na bancada, um ajudante de 24/09 seguia de pé em 02/10, depois de
//! várias atualizações: lia o canal novo do serviço pela metade, e o que chegava do Linux nunca ia
//! para o clipboard do Windows ([log 55](../../../../docs/logs/55-o-manifesto-que-nao-cabia.md)).
//!
//! Sair ao receber um quadro que não entende (`e_incompativel`) não bastava: só pega a mensagem que
//! mudou de forma, e só quando ela chega. Aqui a pergunta é outra, e vale para qualquer mudança:
//! **o executável em disco ainda é o que este processo carregou?** Se não for, este processo é de
//! antes da atualização e sai — e quem zela pelo ajudante (o serviço no Windows, o `systemd` no
//! Linux) sobe o instalado.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, SystemTime};

use tracing::info;

use super::Evento;

/// De quanto em quanto tempo se olha o executável. Ler os metadados de um arquivo é barato, e
/// cinco segundos depois de atualizar o ajudante novo já está de pé.
const INTERVALO: Duration = Duration::from_secs(5);

/// O executável deste processo, como estava quando ele começou.
#[derive(Debug, Clone)]
pub(super) struct Executavel {
    /// O caminho lido na subida. Guardado, e não lido de novo: no Linux, depois de o pacote trocar
    /// o arquivo, o caminho do processo passa a terminar em "(deleted)".
    caminho: PathBuf,
    carimbo: Carimbo,
}

/// O que distingue um arquivo do que o substituiu: tamanho e hora de modificação.
type Carimbo = (u64, Option<SystemTime>);

impl Executavel {
    /// O executável deste processo. `None` se o sistema não disser qual é — aí não há o que vigiar.
    pub(super) fn este() -> Option<Self> {
        Self::de(std::env::current_exe().ok()?)
    }

    fn de(caminho: PathBuf) -> Option<Self> {
        let carimbo = carimbo(&caminho)?;
        Some(Self { caminho, carimbo })
    }

    /// Se o arquivo no caminho já não é o que este processo carregou.
    ///
    /// Sem arquivo nenhum não é mudança: no meio da instalação ele some por um instante, e sair
    /// nessa hora deixaria o zelador sem o que subir.
    pub(super) fn mudou(&self) -> bool {
        carimbo(&self.caminho).is_some_and(|agora| agora != self.carimbo)
    }
}

fn carimbo(caminho: &std::path::Path) -> Option<Carimbo> {
    let dados = std::fs::metadata(caminho).ok()?;
    Some((dados.len(), dados.modified().ok()))
}

/// Olha o executável numa thread própria e avisa o laço principal quando ele mudar.
pub(super) fn vigiar(executavel: Executavel, eventos: Sender<Evento>) {
    thread::spawn(move || {
        loop {
            thread::sleep(INTERVALO);
            if executavel.mudou() {
                info!(
                    "o executável do ajudante foi atualizado: este processo é de antes e sai, para subir o instalado"
                );
                let _ = eventos.send(Evento::Incompativel);
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn arquivo(nome: &str, conteudo: &[u8]) -> PathBuf {
        let caminho =
            std::env::temp_dir().join(format!("ir-executavel-{nome}-{}", std::process::id()));
        std::fs::write(&caminho, conteudo).unwrap();
        caminho
    }

    #[test]
    fn o_mesmo_arquivo_nao_mudou() {
        let caminho = arquivo("igual", b"versao 1");
        let executavel = Executavel::de(caminho.clone()).unwrap();
        assert!(!executavel.mudou());
        let _ = std::fs::remove_file(caminho);
    }

    #[test]
    fn o_arquivo_trocado_pela_atualizacao_mudou() {
        let caminho = arquivo("trocado", b"versao 1");
        let executavel = Executavel::de(caminho.clone()).unwrap();
        std::fs::write(&caminho, b"versao 2, maior").unwrap();
        assert!(executavel.mudou());
        let _ = std::fs::remove_file(caminho);
    }

    #[test]
    fn o_arquivo_que_sumiu_no_meio_da_instalacao_ainda_nao_e_mudanca() {
        let caminho = arquivo("sumiu", b"versao 1");
        let executavel = Executavel::de(caminho.clone()).unwrap();
        std::fs::remove_file(&caminho).unwrap();
        assert!(!executavel.mudou());
    }
}
