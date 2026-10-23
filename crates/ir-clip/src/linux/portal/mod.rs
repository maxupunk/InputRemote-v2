//! O backend do GNOME: o clipboard pelo portal `org.freedesktop.portal.Clipboard`.
//!
//! O GNOME não expõe `data-control`, e o `wl-clipboard` sem ele enche o dock de ícones e a tela de
//! "InputRemote está pronto" ([ADR-0017](../../../../../docs/adr/0017-clipboard-do-gnome-pelo-portal.md)).
//! O portal é o caminho que o GNOME oferece a quem controla a máquina de longe, e não abre janela
//! nenhuma: medido no spike, zero notificações e zero ícones.
//!
//! # Uma sessão só, para quem lê, quem publica e quem vigia
//!
//! [`Clipboard`] e [`Vigia`] são objetos separados, criados em threads diferentes; a sessão do
//! portal, não — cada sessão nova seria um diálogo. Então os dois apontam para um mesmo [`Portal`],
//! que vive o processo inteiro, com duas threads próprias:
//!
//! ```text
//! zelador   abre a sessão (diálogo só na primeira vez), espera ela fechar, reabre
//! escuta    SelectionOwnerChanged → guarda os tipos e acorda o vigia
//!           SelectionTransfer     → alguém colou o que publicamos: serve os bytes
//! ```
//!
//! # O que o portal não tem, e como se contorna
//!
//! **Não há "listar os tipos".** Eles só chegam no aviso de mudança. Antes do primeiro aviso, a
//! leitura tenta os tipos que nos interessam, um por um ([`tipos::TENTATIVAS`]).
//!
//! **Publicar é prometer.** O `SetSelection` só anuncia os tipos; os bytes são pedidos quando alguém
//! cola. Por isso o que publicamos fica guardado aqui até outro programa tomar a seleção.

use std::fs::File;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::{Condvar, Mutex, MutexGuard, Once, OnceLock, PoisonError};
use std::thread;
use std::time::Duration;

use tracing::{debug, info, warn};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedFd, OwnedObjectPath, OwnedValue};

use crate::conteudo::Conteudo;
use crate::error::{ClipError, Result};
pub(super) use acesso::{abrir, vigiar};

mod acesso;
mod sessao;
mod tipos;

use sessao::{CAMINHO, CLIPBOARD, DESTINO, Recusa};

/// O prazo de cada leitura. Ler depende do programa **dono** da seleção responder; se ele travou,
/// esperar mais não o destrava. O mesmo do `wl-paste`.
const PRAZO: Duration = Duration::from_secs(3);

/// A primeira espera depois de uma sessão que não abriu, e o teto dela.
const ESPERA_INICIAL: Duration = Duration::from_secs(2);
const ESPERA_MAXIMA: Duration = Duration::from_secs(15);

/// A sessão do portal deste processo, e o que se sabe do clipboard.
#[derive(Debug)]
pub(super) struct Portal {
    conexao: Connection,
    estado: Mutex<Estado>,
    /// Acorda o vigia quando outro programa copiou, ou quando o portal desistiu.
    mudou: Condvar,
}

#[derive(Debug, Default)]
struct Estado {
    /// A sessão aprovada. `None` enquanto abre, ou depois de fechar e antes de reabrir.
    sessao: Option<OwnedObjectPath>,
    /// A pessoa recusou o diálogo: não há clipboard até o ajudante reiniciar.
    recusado: bool,
    /// Os tipos do último aviso de mudança de outro programa. Vazio: ainda não se sabe.
    oferecidos: Vec<String>,
    /// O que publicamos, enquanto a seleção for nossa.
    nosso: Option<Publicado>,
    /// Quantas vezes outro programa copiou. O vigia compara com o que já viu.
    mudancas: u64,
}

#[derive(Debug)]
struct Publicado {
    conteudo: Conteudo,
    ofertas: Vec<(&'static str, Vec<u8>)>,
}

impl Portal {
    /// O portal deste processo, se o portal do sistema tiver o clipboard.
    ///
    /// Conecta e confere uma vez só; as threads começam no primeiro [`abrir`] ou [`vigiar`].
    pub(super) fn compartilhado() -> Option<&'static Self> {
        static PORTAL: OnceLock<Option<Portal>> = OnceLock::new();
        PORTAL
            .get_or_init(|| {
                let conexao = Connection::session()
                    .inspect_err(|erro| debug!(%erro, "sem barramento da sessão"))
                    .ok()?;
                // Antes de qualquer outra chamada: o portal fixa quem somos na primeira.
                sessao::registrar(&conexao);
                let clipboard = Proxy::new(&conexao, DESTINO, CAMINHO, CLIPBOARD).ok()?;
                let versao = clipboard.get_property::<u32>("version").ok()?;
                debug!(versao, "o portal tem o clipboard");
                Some(Self {
                    conexao,
                    estado: Mutex::new(Estado::default()),
                    mudou: Condvar::new(),
                })
            })
            .as_ref()
    }

    fn estado(&self) -> MutexGuard<'_, Estado> {
        self.estado.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sobe o zelador e a escuta, uma vez por processo.
    fn comecar(&'static self) {
        static COMECOU: Once = Once::new();
        COMECOU.call_once(|| {
            thread::spawn(move || self.escutar());
            thread::spawn(move || self.zelar());
        });
    }

    /// Mantém uma sessão aberta enquanto o processo viver.
    fn zelar(&'static self) {
        let mut espera = ESPERA_INICIAL;
        loop {
            match sessao::abrir(&self.conexao) {
                Ok(aberta) => {
                    info!("clipboard do portal pronto");
                    espera = ESPERA_INICIAL;
                    self.abriu(&aberta.caminho);
                    // Bloqueia até o portal fechar a sessão: tela bloqueada, portal reiniciado, a
                    // pessoa parou o compartilhamento pelo ícone do GNOME.
                    let _ = { aberta.fechou }.next();
                    let mut estado = self.estado();
                    estado.sessao = None;
                    estado.nosso = None;
                    estado.oferecidos.clear();
                    drop(estado);
                    info!("a sessão do portal fechou; reabrindo");
                }
                Err(Recusa::PelaPessoa) => {
                    warn!(
                        "o clipboard não foi autorizado no diálogo do GNOME; copiar e colar fica parado até o próximo login"
                    );
                    self.estado().recusado = true;
                    self.mudou.notify_all();
                    return;
                }
                Err(Recusa::PorAgora(motivo)) => {
                    debug!(%motivo, "a sessão do portal não abriu; tento de novo");
                    thread::sleep(espera);
                    espera = (espera * 2).min(ESPERA_MAXIMA);
                }
            }
        }
    }

    /// Guarda a sessão nova e anuncia o que chegou do par enquanto não havia sessão.
    fn abriu(&self, sessao: &OwnedObjectPath) {
        let pendente: Option<Vec<&'static str>> = {
            let mut estado = self.estado();
            estado.sessao = Some(sessao.clone());
            estado
                .nosso
                .as_ref()
                .map(|nosso| nosso.ofertas.iter().map(|(tipo, _)| *tipo).collect())
        };
        if let Some(tipos) = pendente {
            match self.anunciar(sessao, &tipos) {
                Ok(()) => info!("o que chegou enquanto a sessão não abria está no clipboard"),
                Err(erro) => warn!(%erro, "não consegui pôr no clipboard o que chegou antes"),
            }
        }
    }

    /// Atende os avisos do clipboard do portal.
    fn escutar(&'static self) {
        let sinais = Proxy::new(&self.conexao, DESTINO, CAMINHO, CLIPBOARD)
            .and_then(|proxy| proxy.receive_all_signals());
        let sinais = match sinais {
            Ok(sinais) => sinais,
            Err(erro) => {
                warn!(%erro, "não consegui ouvir o clipboard do portal");
                return;
            }
        };
        for sinal in sinais {
            let membro = sinal.header().member().map(ToString::to_string);
            match membro.as_deref() {
                Some("SelectionOwnerChanged") => self.dono_mudou(&sinal),
                Some("SelectionTransfer") => self.transferir(&sinal),
                _ => {}
            }
        }
    }

    /// Outro programa copiou, ou a nossa publicação entrou.
    fn dono_mudou(&self, sinal: &zbus::Message) {
        let Ok((sessao, opcoes)) = sinal.body().deserialize::<(
            OwnedObjectPath,
            std::collections::HashMap<String, OwnedValue>,
        )>() else {
            return;
        };
        let mut estado = self.estado();
        if estado.sessao.as_ref() != Some(&sessao) {
            return;
        }
        let somos_donos = opcoes
            .get("session_is_owner")
            .and_then(|valor| bool::try_from(valor).ok())
            .unwrap_or(false);
        if somos_donos {
            return;
        }
        estado.oferecidos = opcoes
            .get("mime_types")
            .and_then(|valor| Vec::<String>::try_from(valor.clone()).ok())
            .unwrap_or_default();
        estado.nosso = None;
        estado.mudancas += 1;
        drop(estado);
        self.mudou.notify_all();
    }

    /// Alguém colou o que publicamos: os bytes vão pelo descritor que o portal dá.
    ///
    /// Numa thread: escrever bloqueia até quem colou ler, e uma imagem grande não pode segurar os
    /// avisos seguintes.
    fn transferir(&'static self, sinal: &zbus::Message) {
        let Ok((sessao, tipo, serie)) =
            sinal.body().deserialize::<(OwnedObjectPath, String, u32)>()
        else {
            return;
        };
        let bytes = {
            let estado = self.estado();
            if estado.sessao.as_ref() != Some(&sessao) {
                return;
            }
            estado.nosso.as_ref().and_then(|nosso| {
                nosso
                    .ofertas
                    .iter()
                    .find(|(oferta, _)| oferta.eq_ignore_ascii_case(&tipo))
                    .map(|(_, bytes)| bytes.clone())
            })
        };
        thread::spawn(move || {
            let entregue = bytes.is_some_and(|bytes| self.escrever(&sessao, serie, &bytes).is_ok());
            if !entregue {
                debug!(%tipo, "não entreguei a colagem pedida");
            }
            let _ = self.conexao.call_method(
                Some(DESTINO),
                CAMINHO,
                Some(CLIPBOARD),
                "SelectionWriteDone",
                &(&sessao, serie, entregue),
            );
        });
    }

    fn escrever(&self, sessao: &OwnedObjectPath, serie: u32, bytes: &[u8]) -> Result<()> {
        let resposta = self
            .conexao
            .call_method(
                Some(DESTINO),
                CAMINHO,
                Some(CLIPBOARD),
                "SelectionWrite",
                &(sessao, serie),
            )
            .map_err(falha)?;
        let descritor: OwnedFd = resposta.body().deserialize().map_err(falha)?;
        let mut destino = File::from(std::os::fd::OwnedFd::from(descritor));
        destino
            .write_all(bytes)
            .map_err(|erro| ClipError::Sistema(erro.to_string()))
        // O descritor fecha aqui, e é isso que diz a quem colou que acabou.
    }

    /// Lê um tipo da seleção de outro programa, com prazo.
    fn ler_tipo(&self, sessao: &OwnedObjectPath, tipo: &str) -> Result<Vec<u8>> {
        let resposta = self
            .conexao
            .call_method(
                Some(DESTINO),
                CAMINHO,
                Some(CLIPBOARD),
                "SelectionRead",
                &(sessao, tipo),
            )
            .map_err(falha)?;
        let descritor: OwnedFd = resposta.body().deserialize().map_err(falha)?;
        let mut origem = File::from(std::os::fd::OwnedFd::from(descritor));
        // A leitura numa thread, para o prazo valer: um dono travado prenderia o `read` para sempre.
        // Vencido o prazo, ela fica para trás e termina quando ele fechar o descritor.
        let (pronto, chegou) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pronto.send(origem.read_to_end(&mut bytes).map(|_| bytes));
        });
        match chegou.recv_timeout(PRAZO) {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(erro)) => Err(ClipError::Sistema(erro.to_string())),
            Err(_) => Err(ClipError::Sistema(
                "o programa dono do clipboard não respondeu".to_owned(),
            )),
        }
    }

    /// A sessão, ou por que não há.
    fn sessao(estado: &Estado) -> Result<OwnedObjectPath> {
        if estado.recusado {
            return Err(ClipError::Indisponivel(
                "o clipboard não foi autorizado no diálogo do GNOME",
            ));
        }
        estado.sessao.clone().ok_or(ClipError::Indisponivel(
            "a sessão do portal ainda não está aberta",
        ))
    }
}

fn falha(erro: impl std::fmt::Display) -> ClipError {
    ClipError::Sistema(erro.to_string())
}
