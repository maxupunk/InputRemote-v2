//! A sessão de área de trabalho remota que dá acesso ao clipboard: pedir, lembrar e fechar.
//!
//! O portal só entrega o clipboard dentro de uma sessão `RemoteDesktop` que a pessoa aprovou num
//! diálogo do GNOME. Aprovada uma vez, ela é **lembrada**: o `Start` devolve um `restore_token`, e
//! apresentá-lo na próxima sessão a restaura sem diálogo.
//!
//! # Por que pedir o teclado, se o teclado não é usado
//!
//! O GNOME só guarda uma sessão que compartilha alguma coisa: `serialize_session_as_restore_data`,
//! no `xdg-desktop-portal-gnome` 50, devolve nada quando não há dispositivo nem tela. Pedindo só o
//! clipboard (`types: 0`), o `Start` volta **sem** `restore_token` — foi o que o spike mediu — e o
//! diálogo apareceria a cada login. Então pede-se o teclado, o menor dispositivo, e nunca se
//! manda tecla por ele: quem injeta é o `uinput`, pelo serviço.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use zbus::blocking::proxy::{Builder, SignalIterator};
use zbus::blocking::{Connection, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// O portal, no barramento da sessão.
pub(super) const DESTINO: &str = "org.freedesktop.portal.Desktop";
/// O objeto que tem todas as interfaces do portal.
pub(super) const CAMINHO: &str = "/org/freedesktop/portal/desktop";
/// O clipboard do portal: só funciona dentro de uma sessão aprovada.
pub(super) const CLIPBOARD: &str = "org.freedesktop.portal.Clipboard";
const AREA_REMOTA: &str = "org.freedesktop.portal.RemoteDesktop";
const SESSAO: &str = "org.freedesktop.portal.Session";

/// Só o teclado: o mínimo para a sessão ser lembrada (ver o começo do módulo).
const TECLADO: u32 = 1;
/// Lembrar até a pessoa revogar.
const PERSISTENTE: u32 = 2;

/// O nome com que o portal nos conhece: o do arquivo `.desktop` instalado. É a ele que o GNOME
/// liga a autorização lembrada, e é ele que aparece no diálogo.
pub(super) const APLICATIVO: &str = "inputremote";

/// Por que a sessão não abriu.
#[derive(Debug)]
pub(super) enum Recusa {
    /// A pessoa disse não no diálogo, ou desligou o clipboard nele. Não se pergunta de novo até o
    /// ajudante reiniciar: perguntar em laço seria insistir contra uma resposta dada.
    PelaPessoa,
    /// Algo passageiro: tela bloqueada (*"Session creation inhibited"*), portal reiniciando.
    PorAgora(String),
}

impl From<zbus::Error> for Recusa {
    fn from(erro: zbus::Error) -> Self {
        Self::PorAgora(erro.to_string())
    }
}

/// Uma sessão aprovada, e o aviso de quando ela fechar.
pub(super) struct Aberta {
    pub(super) caminho: OwnedObjectPath,
    /// O sinal `Closed` da sessão, assinado **antes** do `Start`: assinar depois perderia um
    /// fechamento que acontecesse no meio.
    pub(super) fechou: SignalIterator<'static>,
}

/// Diz ao portal quem somos, antes de qualquer outra chamada desta conexão.
///
/// O ajudante roda numa unidade do `systemd` cujo nome não diz o aplicativo, e sem isto o portal o
/// trataria como anônimo. Portais anteriores ao 1.19 não têm o registro; aí seguimos anônimos, e a
/// autorização é lembrada do mesmo jeito.
pub(super) fn registrar(conexao: &Connection) {
    let opcoes: HashMap<&str, Value<'_>> = HashMap::new();
    if let Err(erro) = conexao.call_method(
        Some(DESTINO),
        CAMINHO,
        Some("org.freedesktop.host.portal.Registry"),
        "Register",
        &(APLICATIVO, opcoes),
    ) {
        tracing::debug!(%erro, "o portal não tem o registro de aplicativo; seguimos anônimos");
    }
}

/// Abre uma sessão com o clipboard, restaurando a autorização lembrada se houver.
///
/// Sem autorização lembrada, o GNOME mostra o diálogo e isto espera a resposta da pessoa.
pub(super) fn abrir(conexao: &Connection) -> Result<Aberta, Recusa> {
    esperar_a_tela(conexao);
    let criada = pedido(conexao, "CreateSession", |ficha| {
        let opcoes = HashMap::from([
            ("handle_token", Value::from(ficha)),
            ("session_handle_token", Value::from(format!("s{ficha}"))),
        ]);
        conexao.call_method(
            Some(DESTINO),
            CAMINHO,
            Some(AREA_REMOTA),
            "CreateSession",
            &(opcoes,),
        )
    })?;
    let caminho = criada
        .get("session_handle")
        .and_then(|valor| String::try_from(valor.clone()).ok())
        .and_then(|texto| OwnedObjectPath::try_from(texto).ok())
        .ok_or_else(|| Recusa::PorAgora("o portal não disse qual é a sessão".to_owned()))?;
    let fechou = assinar_fechamento(conexao, &caminho)?;
    match aprovar(conexao, &caminho) {
        Ok(()) => Ok(Aberta { caminho, fechou }),
        Err(recusa) => {
            fechar(conexao, &caminho);
            Err(recusa)
        }
    }
}

/// Espera a proteção de tela sair, se ela estiver de pé.
///
/// Com a tela bloqueada ou apagada o GNOME recusa sessão nova (*"Session creation inhibited"*,
/// medido no spike), e a recusa custa caro: o `xdg-desktop-portal` **consome** a ficha lembrada ao
/// recebê-la no `SelectDevices`, antes de o GNOME recusar. Tentar com a tela bloqueada apagaria a
/// autorização, e o diálogo voltaria. Então nem se tenta: espera-se o aviso de que a tela voltou.
///
/// Fora do GNOME, ou sem a interface, segue direto.
fn esperar_a_tela(conexao: &Connection) {
    const PROTECAO: &str = "org.gnome.ScreenSaver";
    let Ok(protecao) = Proxy::new(conexao, PROTECAO, "/org/gnome/ScreenSaver", PROTECAO) else {
        return;
    };
    // Assinado antes de perguntar: a tela pode voltar entre a pergunta e a assinatura.
    let Ok(mudancas) = protecao.receive_signal("ActiveChanged") else {
        return;
    };
    let ativa = protecao
        .call::<_, _, bool>("GetActive", &())
        .unwrap_or(false);
    if !ativa {
        return;
    }
    tracing::info!("tela bloqueada: o clipboard do portal espera ela voltar");
    for aviso in mudancas {
        if matches!(aviso.body().deserialize::<bool>(), Ok(false)) {
            return;
        }
    }
}

/// Escolhe o dispositivo, pede o clipboard e começa: o passo em que o diálogo aparece.
fn aprovar(conexao: &Connection, caminho: &OwnedObjectPath) -> Result<(), Recusa> {
    let lembrada = ficha_lembrada::ler();
    pedido(conexao, "SelectDevices", |ficha| {
        let mut opcoes = HashMap::from([
            ("handle_token", Value::from(ficha)),
            ("types", Value::from(TECLADO)),
            ("persist_mode", Value::from(PERSISTENTE)),
        ]);
        if let Some(lembrada) = &lembrada {
            opcoes.insert("restore_token", Value::from(lembrada.as_str()));
        }
        conexao.call_method(
            Some(DESTINO),
            CAMINHO,
            Some(AREA_REMOTA),
            "SelectDevices",
            &(caminho, opcoes),
        )
    })?;
    let sem_opcoes: HashMap<&str, Value<'_>> = HashMap::new();
    conexao.call_method(
        Some(DESTINO),
        CAMINHO,
        Some(CLIPBOARD),
        "RequestClipboard",
        &(caminho, sem_opcoes),
    )?;
    let resultado = pedido(conexao, "Start", |ficha| {
        let opcoes = HashMap::from([("handle_token", Value::from(ficha))]);
        conexao.call_method(
            Some(DESTINO),
            CAMINHO,
            Some(AREA_REMOTA),
            "Start",
            &(caminho, "", opcoes),
        )
    });
    concluir(resultado)
}

/// Lembra a autorização que o `Start` devolveu, e confere que ela inclui o clipboard.
fn concluir(resultado: Result<HashMap<String, OwnedValue>, Recusa>) -> Result<(), Recusa> {
    let resultado = match resultado {
        Ok(resultado) => resultado,
        Err(recusa) => {
            // Uma ficha apresentada foi consumida pelo portal, e não vale mais.
            ficha_lembrada::esquecer();
            return Err(recusa);
        }
    };
    match resultado
        .get("restore_token")
        .and_then(|valor| String::try_from(valor.clone()).ok())
    {
        Some(nova) => ficha_lembrada::guardar(&nova),
        None => ficha_lembrada::esquecer(),
    }
    let com_clipboard = resultado
        .get("clipboard_enabled")
        .and_then(|valor| bool::try_from(valor).ok())
        .unwrap_or(false);
    if com_clipboard {
        Ok(())
    } else {
        Err(Recusa::PelaPessoa)
    }
}

/// Fecha a sessão. Sem resposta a esperar: se o portal já a fechou, não há o que fazer.
pub(super) fn fechar(conexao: &Connection, caminho: &OwnedObjectPath) {
    let _ = conexao.call_method(Some(DESTINO), caminho, Some(SESSAO), "Close", &());
}

fn assinar_fechamento(
    conexao: &Connection,
    caminho: &OwnedObjectPath,
) -> Result<SignalIterator<'static>, Recusa> {
    let sessao: Proxy<'static> = Builder::new(conexao)
        .destination(DESTINO)?
        .path(caminho.clone())?
        .interface(SESSAO)?
        .cache_properties(CacheProperties::No)
        .build()?;
    Ok(sessao.receive_signal("Closed")?)
}

/// Uma chamada que o portal responde depois, pelo sinal `Response` de um objeto `Request`.
///
/// O caminho do `Request` é previsível — o nome desta conexão e a ficha —, e a assinatura é feita
/// **antes** da chamada: uma resposta rápida, como a de uma sessão restaurada, não se perde.
fn pedido(
    conexao: &Connection,
    metodo: &str,
    chamar: impl FnOnce(&str) -> zbus::Result<zbus::Message>,
) -> Result<HashMap<String, OwnedValue>, Recusa> {
    static CONTADOR: AtomicU32 = AtomicU32::new(0);
    let ficha = format!(
        "ir{}_{}",
        std::process::id(),
        CONTADOR.fetch_add(1, Ordering::Relaxed)
    );
    let remetente = conexao
        .unique_name()
        .map(|nome| nome.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    let pedido: Proxy<'static> = Builder::new(conexao)
        .destination(DESTINO)?
        .path(format!("{CAMINHO}/request/{remetente}/{ficha}"))?
        .interface("org.freedesktop.portal.Request")?
        .cache_properties(CacheProperties::No)
        .build()?;
    let mut respostas = pedido.receive_signal("Response")?;
    chamar(&ficha)?;
    let resposta = respostas
        .next()
        .ok_or_else(|| Recusa::PorAgora(format!("{metodo}: o portal saiu sem responder")))?;
    let (codigo, resultados): (u32, HashMap<String, OwnedValue>) = resposta.body().deserialize()?;
    match codigo {
        0 => Ok(resultados),
        1 => Err(Recusa::PelaPessoa),
        outro => Err(Recusa::PorAgora(format!("{metodo} respondeu {outro}"))),
    }
}

/// A ficha que restaura a autorização, guardada entre execuções.
mod ficha_lembrada {
    use super::PathBuf;

    /// No estado do usuário, junto do resto do InputRemote: não é configuração, e perdê-la só custa
    /// um diálogo.
    fn caminho() -> Option<PathBuf> {
        let estado = std::env::var_os("XDG_STATE_HOME")
            .filter(|valor| !valor.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|casa| PathBuf::from(casa).join(".local/state"))
            })?;
        Some(estado.join("inputremote").join("portal-clipboard"))
    }

    pub(super) fn ler() -> Option<String> {
        let texto = std::fs::read_to_string(caminho()?).ok()?;
        let ficha = texto.trim();
        (!ficha.is_empty()).then(|| ficha.to_owned())
    }

    pub(super) fn guardar(ficha: &str) {
        let Some(caminho) = caminho() else { return };
        let gravou = caminho
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&caminho, ficha));
        if let Err(erro) = gravou {
            tracing::warn!(%erro, "não guardei a autorização do portal; o diálogo volta no próximo login");
        }
    }

    pub(super) fn esquecer() {
        if let Some(caminho) = caminho() {
            let _ = std::fs::remove_file(caminho);
        }
    }
}
