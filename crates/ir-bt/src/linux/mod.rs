//! O rádio do Linux: sockets `AF_BLUETOOTH` do kernel, sem D-Bus.
//!
//! Usa o `bluer` com **apenas** o recurso `rfcomm`, que é Rust puro sobre os sockets do kernel e
//! não fala com o `bluetoothd`. A documentação do próprio módulo diz que ele "não registra nem
//! usa registros SDP" — que é exatamente a decisão do
//! [ADR-0009](../../../../docs/adr/0009-canal-rfcomm-fixo-sem-sdp.md), e o motivo de o RPM
//! continuar sem dependência nativa nenhuma.
//!
//! # Sem D-Bus, quem sabe dos pares é o disco
//!
//! Sem `bluetoothd` não há `org.bluez.Device1` para perguntar quem está pareado. A resposta está
//! onde o BlueZ a grava: `/var/lib/bluetooth/<adaptador>/<dispositivo>/info`. Ler esse diretório
//! exige ser root — e o serviço é root, porque precisa de `/dev/uinput` de qualquer forma
//! ([06, §2](../../../../docs/06-linux.md)). A leitura do formato fica em
//! [`bluez`](crate::bluez), separada e testável em qualquer plataforma.

use std::path::Path;
// Só a bancada de testes monta caminhos; o código de produção apenas os percorre.
#[cfg(test)]
use std::path::PathBuf;

use bluer::Address;
use bluer::rfcomm::{Listener, SocketAddr, Stream};

use crate::addr::{BdAddr, CANAL};
use crate::bluez;
use crate::error::{BtError, Result};
use crate::radio::{Dispositivo, Radio};

/// Onde o BlueZ guarda os pares conhecidos.
const ARMAZENAMENTO: &str = "/var/lib/bluetooth";

/// Onde o kernel lista os adaptadores presentes.
const ADAPTADORES: &str = "/sys/class/bluetooth";

/// Quantas conexões o sistema enfileira enquanto não as aceitamos.
///
/// Uma sessão é ponto a ponto: uma conexão em curso e uma na fila bastam para uma tentativa
/// repetida do par não ser recusada pelo sistema.
const FILA: u32 = 2;

/// O rádio Bluetooth desta máquina Linux.
#[derive(Debug)]
pub struct RadioLinux {
    escuta: Listener,
}

impl RadioLinux {
    /// Abre a escuta no canal do produto.
    ///
    /// # Errors
    ///
    /// [`BtError::SemRadio`] se não houver adaptador; [`BtError::Io`] se o canal já estiver
    /// ocupado por outro programa — o caso que o [ADR-0009] assume e manda dizer em voz alta, em
    /// vez de sair procurando outro canal e fazer as duas pontas discordarem.
    ///
    /// [ADR-0009]: ../../../../docs/adr/0009-canal-rfcomm-fixo-sem-sdp.md
    pub fn abrir() -> Result<Self> {
        if !ha_adaptador(Path::new(ADAPTADORES)) {
            return Err(BtError::SemRadio(
                "nenhum adaptador Bluetooth em /sys/class/bluetooth".to_owned(),
            ));
        }
        let socket = bluer::rfcomm::Socket::new()?;
        socket.bind(SocketAddr::new(Address::any(), CANAL))?;
        let escuta = socket.listen(FILA)?;
        Ok(Self { escuta })
    }
}

impl Radio for RadioLinux {
    type Canal = Stream;

    async fn disponivel(&self) -> bool {
        ha_adaptador(Path::new(ADAPTADORES))
    }

    async fn pareados(&self) -> Result<Vec<Dispositivo>> {
        // Passeio por diretório é E/S bloqueante; fora da thread do runtime.
        tokio::task::spawn_blocking(|| ler_pareados(Path::new(ARMAZENAMENTO)))
            .await
            .map_err(|erro| BtError::SemRadio(erro.to_string()))?
    }

    async fn conectar(&self, alvo: BdAddr) -> Result<Self::Canal> {
        let destino = SocketAddr::new(Address(alvo.bytes()), CANAL);
        Stream::connect(destino)
            .await
            .map_err(|erro| traduzir(&erro, alvo))
    }

    async fn aceitar(&self) -> Result<(Self::Canal, BdAddr)> {
        let (fluxo, origem) = self.escuta.accept().await?;
        Ok((fluxo, BdAddr(origem.addr.0)))
    }
}

/// Se o kernel enxerga algum adaptador.
///
/// `/sys/class/bluetooth` existe mesmo sem rádio nenhum; o que conta é haver uma entrada `hci`
/// dentro dele.
fn ha_adaptador(raiz: &Path) -> bool {
    let Ok(entradas) = std::fs::read_dir(raiz) else {
        return false;
    };
    entradas.flatten().any(|entrada| {
        entrada
            .file_name()
            .to_str()
            .is_some_and(|nome| nome.starts_with("hci"))
    })
}

/// Lê os pares gravados pelo BlueZ, sob todos os adaptadores.
fn ler_pareados(raiz: &Path) -> Result<Vec<Dispositivo>> {
    let adaptadores = std::fs::read_dir(raiz).map_err(|erro| {
        // Não conseguir ler aqui quase sempre é falta de privilégio, e essa é uma informação
        // acionável — bem diferente de "não há pares".
        BtError::SemRadio(format!("{} não pôde ser lido: {erro}", raiz.display()))
    })?;

    let mut encontrados = Vec::new();
    for adaptador in adaptadores.flatten() {
        if !adaptador.path().is_dir() {
            continue;
        }
        recolher_do_adaptador(&adaptador.path(), &mut encontrados);
    }
    Ok(encontrados)
}

/// Junta os pares de um adaptador à lista.
fn recolher_do_adaptador(adaptador: &Path, encontrados: &mut Vec<Dispositivo>) {
    let Ok(dispositivos) = std::fs::read_dir(adaptador) else {
        return;
    };
    for dispositivo in dispositivos.flatten() {
        let caminho = dispositivo.path();
        let Some(endereco) = endereco_do_caminho(&caminho) else {
            continue; // subdiretório que não é um par (`cache`, `settings`, ...)
        };
        let Ok(texto) = std::fs::read_to_string(caminho.join("info")) else {
            continue;
        };
        let info = bluez::ler_info(&texto);
        if !info.pareado_por_bredr {
            // Conhecido não é pareado, e sem BR/EDR não há RFCOMM. Oferecê-lo na tela seria
            // oferecer uma escolha que não pode funcionar.
            continue;
        }
        encontrados.push(Dispositivo {
            endereco,
            nome: info.nome.unwrap_or_else(|| endereco.to_string()),
            // Sem D-Bus não dá para saber se o rádio está ligado a ele agora. Dizer "não" é
            // honesto: o sistema conecta sozinho quando alguém abre um canal.
            conectado: false,
            classe: info.classe,
        });
    }
}

/// O endereço que dá nome ao diretório, se ele for mesmo um endereço.
fn endereco_do_caminho(caminho: &Path) -> Option<BdAddr> {
    caminho.file_name()?.to_str()?.parse().ok()
}

/// Traduz a falha de conexão no que o usuário precisa ouvir.
fn traduzir(erro: &std::io::Error, alvo: BdAddr) -> BtError {
    use std::io::ErrorKind;

    /// `EBUSY`: o kernel já tem uma sessão RFCOMM com este par neste canal.
    const OCUPADO: i32 = 16;

    if erro.raw_os_error() == Some(OCUPADO) {
        // Não é falta de par nem falta de resposta: é sobra de uma conexão anterior que o enlace
        // de baixo nível ainda segura (log 28). Chamar isto de "erro de socket" mandava a pessoa
        // procurar defeito onde não há.
        return BtError::Ocupado(alvo.to_string());
    }

    match erro.kind() {
        // O par existe e está pareado, mas ninguém atende no canal do produto.
        ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset | ErrorKind::TimedOut => {
            BtError::SemResposta
        }
        // O kernel não tem vínculo com este endereço.
        ErrorKind::HostUnreachable | ErrorKind::NotConnected => {
            BtError::NaoPareado(alvo.to_string())
        }
        _ => BtError::Io(std::io::Error::new(erro.kind(), erro.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um armazenamento do BlueZ de mentira, montado em disco.
    fn armazenamento(nome: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("ir-bluez-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        raiz
    }

    fn gravar_par(adaptador: &Path, endereco: &str, conteudo: &str) {
        let dir = adaptador.join(endereco);
        std::fs::create_dir_all(&dir).expect("cria o diretório do par");
        std::fs::write(dir.join("info"), conteudo).expect("grava o info");
    }

    #[test]
    fn so_entram_na_lista_os_pares_com_chave_de_bredr() {
        let raiz = armazenamento("lista");
        let adaptador = raiz.join("AC:50:DE:47:EB:28");
        gravar_par(
            &adaptador,
            "74:13:EA:A6:5A:99",
            "[General]\nName=SAMSUNG-MAXUEL\nSupportedTechnologies=BR/EDR;\n\n[LinkKey]\nKey=00\n",
        );
        gravar_par(
            &adaptador,
            "11:22:33:44:55:66",
            "[General]\nName=Fone\nSupportedTechnologies=LE;\n\n[LongTermKey]\nKey=00\n",
        );
        // Um diretório que não é par nenhum, como o `cache` que o BlueZ cria.
        std::fs::create_dir_all(adaptador.join("cache")).expect("cria cache");

        let pareados = ler_pareados(&raiz).expect("lê o armazenamento");

        assert_eq!(pareados.len(), 1, "{pareados:?}");
        let par = pareados.first().expect("um par");
        assert_eq!(par.nome, "SAMSUNG-MAXUEL");
        assert_eq!(par.endereco.to_string(), "74:13:EA:A6:5A:99");

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn um_armazenamento_ilegivel_diz_que_nao_deu_para_ler() {
        // Quase sempre é falta de privilégio. Devolver uma lista vazia mentiria dizendo "não há
        // nenhum par", e o usuário procuraria o problema no lugar errado.
        let erro =
            ler_pareados(Path::new("/var/lib/bluetooth-que-nao-existe")).expect_err("não existe");
        assert!(matches!(erro, BtError::SemRadio(_)), "{erro}");
    }

    #[test]
    fn sem_entrada_hci_nao_ha_radio() {
        let raiz = armazenamento("sem-hci");
        std::fs::create_dir_all(&raiz).expect("cria");
        assert!(!ha_adaptador(&raiz), "diretório vazio não é rádio");
        std::fs::create_dir_all(raiz.join("hci0")).expect("cria hci0");
        assert!(ha_adaptador(&raiz));
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn conexao_recusada_vira_sem_resposta_e_nao_erro_cru() {
        let alvo = BdAddr([1, 2, 3, 4, 5, 6]);
        let recusada = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        assert!(matches!(traduzir(&recusada, alvo), BtError::SemResposta));
    }
}
