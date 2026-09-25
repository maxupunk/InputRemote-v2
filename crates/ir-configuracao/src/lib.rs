//! Configuração e identidade persistentes da máquina.
//!
//! Tudo pertence à máquina, não ao usuário ([02, §7](../../../docs/02-arquitetura.md)). O diretório
//! de estado é decidido num lugar só ([`data_dir`]): `IR_DATA_DIR` quando vem de fora (a unidade do
//! `systemd` e os testes), `%ProgramData%\InputRemote` no serviço do Windows, e `./ir-state` no teste
//! em primeiro plano — o que evita depender de privilégio antes da instalação como serviço.
//!
//! Saiu do `ir-daemon` quando a rota dupla o levou ao teto de tamanho de crate
//! ([ADR-0012](../../../docs/adr/0012-rota-dupla.md)): o que a máquina guarda em disco já era uma
//! fronteira estável, que só conhecia a identidade, a política e a borda.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

mod gravador;
mod politica;

pub use gravador::Gravador;
pub use politica::{
    edge_do_texto, edge_para_texto, politica_do_texto, politica_na_subida, politica_sustentada,
    portador_do_texto, texto_da_politica, texto_do_portador,
};

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ir_crypto::{Identity, PublicKey};
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::Policy;
use serde::{Deserialize, Serialize};

/// A configuração da máquina, como fica no `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Quem pode controlar quem: `ambos` (o padrão), `so-este` (este controla o outro e nunca é
    /// controlado) ou `so-o-outro` (este é controlado e nunca controla).
    ///
    /// Um arquivo de antes do controle simétrico tem `role`, que é ignorado: a máquina sobe com os
    /// dois controlando um ao outro (ADR-0014).
    #[serde(default = "politica_padrao")]
    pub politica: String,
    /// A borda que dá para o par: `left`/`right`/`top`/`bottom`.
    pub peer_edge: String,
    /// A porta UDP local.
    pub port: u16,
    /// Largura da tela em pixels (usada quando a plataforma não informa).
    pub screen_width: u32,
    /// Altura da tela em pixels.
    pub screen_height: u32,
    /// O endereço do par, para iniciar a conexão. Opcional.
    ///
    /// Duas formas, e é ela que decide o portador: `10.0.0.135:52525` fala pela rede,
    /// `AC:50:DE:47:EB:28` fala pelo Bluetooth. As duas não se confundem — um `ip:porta` tem dois
    /// grupos separados por `:`, um endereço de rádio tem seis —, e é por isso que um campo de
    /// texto só dá conta dos dois sem o arquivo de configuração mudar de formato.
    pub peer_addr: Option<String>,
    /// Pares já pareados.
    #[serde(default)]
    pub peers: Vec<PinnedPeer>,
    /// Onde os arquivos recebidos ficam. Vazio significa `<estado>/recebidos`.
    ///
    /// Configurável porque a pasta de estado pode estar num disco pequeno, e uma transferência de
    /// 5 GB não deve ser obrigada a caber junto com a configuração.
    #[serde(default)]
    pub recebidos: Option<String>,
    /// O valor de `SoftwareSASGeneration` antes de o produto o mudar, no Windows.
    ///
    /// Ligar a digitação na tela de bloqueio liga também o Ctrl+Alt+Del gerado pelo serviço, que é
    /// uma política da máquina. Desligar devolve o que estava — e só se foi o produto quem mudou.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub politica_de_atencao_anterior: Option<u32>,
    /// O meio de conexão fixado nas preferências: `bluetooth` ou `rede`. Ausente é automático.
    ///
    /// Gravado, e não só na memória: fixar o Bluetooth para testar e ver a escolha voltar a
    /// "automático" depois de reiniciar parecia a preferência sendo ignorada.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portador_fixado: Option<String>,
    /// Se o outro computador bloqueia junto quando este bloquear a tela. Ligado por padrão: o
    /// computador que era controlado não pode ficar aberto para quem passar por ele.
    #[serde(default = "sim")]
    pub bloquear_juntos: bool,
    /// Quando a borda foi escolhida na tela, em milissegundos desde 1970.
    ///
    /// Os dois computadores anunciam a borda um ao outro; se não forem opostas, vale a escolha mais
    /// recente, e o outro passa a usar a oposta sozinho. Sem isto gravado, a escolha de antes de
    /// reiniciar perderia para qualquer outra.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub borda_escolhida_em: Option<u64>,
}

/// O padrão das opções que nascem ligadas.
const fn sim() -> bool {
    true
}

/// A política de quem não gravou nenhuma: os dois controlando um ao outro.
fn politica_padrao() -> String {
    politica::PADRAO.to_owned()
}

/// Um par pareado, com a chave estática fixada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedPeer {
    /// A chave pública, em hexadecimal.
    pub pubkey: String,
    /// O último endereço conhecido, se houver: o do pareamento, de rede ou de rádio.
    pub addr: Option<String>,
    /// O rádio do par, que ele contou depois de pareado pela rede: a próxima subida já disca o
    /// Bluetooth (ADR-0012). Opcional, e arquivos gravados antes continuam valendo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radio: Option<String>,
    /// O nome que o par deu a si mesmo, para a tela dizer "notebook-da-ana" mesmo desconectado.
    /// Antes a tela dizia "computador pareado", que parecia defeito.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nome: Option<String>,
    /// Se este par está **proibido** de digitar na tela de bloqueio e nos pedidos de permissão
    /// daqui.
    ///
    /// Grava-se a recusa, e não a permissão, porque o padrão é permitir: os dois computadores
    /// controlam um ao outro também na tela de bloqueio (log 53, [04, §6](../../../docs/04-seguranca.md)).
    /// O par só existe depois da comparação dos seis dígitos nas duas telas. Quem quiser proibir
    /// desliga pela janela. Um arquivo antigo tinha `tela_de_bloqueio`, desligado por padrão — ele é
    /// ignorado, e o par passa a poder.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recusa_tela_de_bloqueio: bool,
}

impl PinnedPeer {
    /// Se este par pode digitar na tela de bloqueio e nos pedidos de permissão daqui.
    #[must_use]
    pub const fn permite_tela_de_bloqueio(&self) -> bool {
        !self.recusa_tela_de_bloqueio
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            politica: politica_padrao(),
            peer_edge: "right".to_owned(),
            port: ir_proto::DEFAULT_PORT,
            screen_width: 1920,
            screen_height: 1080,
            peer_addr: None,
            peers: Vec::new(),
            recebidos: None,
            politica_de_atencao_anterior: None,
            portador_fixado: None,
            bloquear_juntos: true,
            borda_escolhida_em: None,
        }
    }
}

impl Config {
    /// Quem pode controlar quem.
    ///
    /// # Errors
    ///
    /// Erro se o texto não for uma das três políticas.
    pub fn policy(&self) -> Result<Policy> {
        politica_do_texto(&self.politica)
    }

    /// A borda de travessia.
    ///
    /// # Errors
    ///
    /// Erro se o texto não nomear uma borda.
    pub fn edge(&self) -> Result<Edge> {
        edge_do_texto(&self.peer_edge)
    }

    /// O portador fixado nas preferências, ou nenhum — a escolha automática.
    #[must_use]
    pub fn fixado(&self) -> Option<Carrier> {
        portador_do_texto(self.portador_fixado.as_deref())
    }

    /// Fixa um portador nas preferências, ou volta à escolha automática.
    pub fn fixar(&mut self, portador: Option<Carrier>) {
        self.portador_fixado = portador.map(|portador| texto_do_portador(portador).to_owned());
    }

    /// Onde o par está, pelo que o arquivo diz: o endereço configurado à mão vence; sem ele, vale o
    /// endereço por onde o par foi pareado.
    #[must_use]
    pub fn endereco_do_par(&self) -> Option<&str> {
        self.peer_addr
            .as_deref()
            .or_else(|| self.peers.first().and_then(|par| par.addr.as_deref()))
    }

    /// Onde os arquivos recebidos ficam.
    #[must_use]
    pub fn pasta_de_recebidos(&self, dir: &Path) -> PathBuf {
        self.recebidos
            .as_deref()
            .filter(|texto| !texto.trim().is_empty())
            .map_or_else(|| recebidos_padrao(dir), PathBuf::from)
    }

    /// A primeira chave de par fixada, se houver.
    #[must_use]
    pub fn first_peer_key(&self) -> Option<PublicKey> {
        self.peers.first().and_then(|p| decode_key(&p.pubkey))
    }

    /// Grava a configuração no diretório de estado, de forma atômica.
    ///
    /// # Errors
    ///
    /// Erro de E/S ao escrever.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("serializando config")?;
        gravar_atomico(dir, "config.toml", text.as_bytes(), false).context("gravando config")
    }
}

/// A pasta de recebidos quando a configuração não escolhe outra: dentro da pasta de estado.
#[must_use]
pub fn recebidos_padrao(dir: &Path) -> PathBuf {
    dir.join("recebidos")
}

/// O diretório de estado.
///
/// `IR_DATA_DIR` vence sempre: é por ele que a unidade do `systemd` aponta `/var/lib/inputremote`,
/// e que os testes e o desenvolvimento escolhem uma pasta própria. Sem ele, o serviço do Windows
/// (`servico_do_windows`) guarda em `%ProgramData%\InputRemote`, que o SYSTEM sabe escrever e nenhum
/// usuário comum adultera; o teste em primeiro plano, em `./ir-state`.
#[must_use]
pub fn data_dir(servico_do_windows: bool) -> PathBuf {
    if let Some(dir) = std::env::var_os("IR_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if servico_do_windows {
        let base = std::env::var_os("ProgramData")
            .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
        return base.join("InputRemote");
    }
    PathBuf::from("ir-state")
}

/// Carrega a configuração, criando o padrão se ainda não existir.
///
/// # Errors
///
/// Erro de E/S ou de formato ao ler o `config.toml`.
pub fn load_config(dir: &Path) -> Result<Config> {
    std::fs::create_dir_all(dir).context("criando o diretório de estado")?;
    let path = dir.join("config.toml");
    if !path.exists() {
        let config = Config::default();
        config.save(dir)?;
        return Ok(config);
    }
    let text = std::fs::read_to_string(&path).context("lendo config")?;
    toml::from_str(&text).context("interpretando config")
}

/// Carrega a identidade da máquina, gerando e gravando na primeira vez.
///
/// # Errors
///
/// Erro de E/S, ou material de chave inválido no arquivo.
pub fn load_identity(dir: &Path) -> Result<Identity> {
    let path = dir.join("identity.key");
    if path.exists() {
        // Uma chave gravada por uma versão antiga, ou mexida à mão, volta a ser só do dono.
        restrict(&path)?;
        let bytes = std::fs::read(&path).context("lendo identidade")?;
        return Identity::from_secret_bytes(&bytes).context("identidade inválida");
    }
    let identity = Identity::generate();
    gravar_atomico(
        dir,
        "identity.key",
        identity.secret_bytes().as_bytes(),
        true,
    )
    .context("gravando identidade")?;
    Ok(identity)
}

/// Grava `nome` em `dir` de forma atômica: um temporário, `sync_all`, e só então a troca.
///
/// Uma rotina só para a configuração e a chave. Eram duas, e só a da chave fazia `sync_all`: uma
/// queda de energia logo depois de gravar a configuração podia deixar um `config.toml` vazio.
///
/// `privado` faz o arquivo **nascer** legível só pelo dono. Gravar e depois restringir deixava um
/// instante em que a chave era legível por todos — e o erro da restrição era ignorado. No Windows
/// quem fecha é o DACL da pasta de estado, aplicado pelo serviço a cada subida (`ir-acesso`).
fn gravar_atomico(dir: &Path, nome: &str, bytes: &[u8], privado: bool) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = dir.join(format!("{nome}.tmp"));
    // Um temporário de uma gravação interrompida não pode impedir esta.
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if privado {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = privado;
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, dir.join(nome))
}

/// Restringe ao dono a permissão de um arquivo de chave que já existe.
///
/// # Errors
///
/// Erro de E/S se a permissão não puder ser trocada: seguir com a chave aberta seria pior.
#[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
fn restrict(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let atual = std::fs::metadata(path)
            .context("lendo a permissão da identidade")?
            .permissions()
            .mode();
        if atual & 0o077 != 0 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .context("restringindo a identidade ao dono")?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Grava a chave pública de um par em hexadecimal.
#[must_use]
pub fn encode_key(key: &PublicKey) -> String {
    ir_proto::ids::Hex(&key.0).to_string()
}

/// Lê uma chave pública de 64 hexadecimais.
#[must_use]
pub fn decode_key(text: &str) -> Option<PublicKey> {
    if text.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (slot, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let hi = (pair.first().copied()?) as char;
        let lo = (pair.get(1).copied()?) as char;
        let hi = u8::try_from(hi.to_digit(16)?).ok()?;
        let lo = u8::try_from(lo.to_digit(16)?).ok()?;
        *slot = (hi << 4) | lo;
    }
    Some(PublicKey(bytes))
}

#[cfg(test)]
mod tests;
