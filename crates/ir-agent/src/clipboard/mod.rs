//! O ajudante de clipboard: `inputremote-agent --clipboard`.
//!
//! # Roda como o usuário, e fala pelo canal de controle
//!
//! O agente de entrada roda como SYSTEM no Windows, lançado pelo serviço, e fala por um canal
//! restrito — porque injeta teclado no prompt de elevação. O clipboard não é nada disso: é dado **do
//! usuário**, na sessão do usuário. Então quem cuida dele roda como o usuário, iniciado pela própria
//! sessão, e fala pelo mesmo canal que a interface usa, com o portão por credencial que já existe
//! ([ADR-0011](../../../docs/adr/0011-clipboard-na-travessia.md)).
//!
//! Isso resolve três coisas de uma vez:
//!
//! - no **Linux**, o canal do agente é `0600 root` e o usuário da sessão não o abre; e liberá-lo
//!   faria o serviço mandar a injeção para quem conectasse, tirando-a do `uinput`;
//! - no **Windows**, clipboard deixa de exigir SYSTEM;
//! - o ajudante não pode pedir nada que o usuário não pudesse pedir pela janela.
//!
//! # O que ele faz
//!
//! ```text
//! o sistema avisa mudança (Windows)  ┐                          ┌► Pedido::EnviarArquivos
//! Aviso::LerClipboard (travessia)    ┴─► lê ─► Eco::oferecer ─┴► Pedido::OferecerTexto
//!
//! Aviso::Transferencia(recebendo, concluída) ┐
//! Aviso::TextoRecebido                       ┴─► Eco::publicamos ─► publica o que chegou
//! ```
//!
//! Nada de política aqui: quando ler, quem decide é o serviço; o que é eco, quem decide é o [`Eco`].
//! Arquivos vão pelo canal de dados (TCP); texto, pelo canal 4 da sessão, em qualquer portador.
//! Texto acima do limite do canal 4 não atravessa, e fica só no registro, sem o conteúdo.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use ir_clip::{Clipboard, Conteudo, Eco};
use ir_ipc::codec;
use ir_ipc::transferencia::{Fase, Sentido, Transferencia};
use ir_ipc::{Aviso, Falha, ParaInterface, Pedido, Resposta, TextoDoClipboard};
use tracing::{debug, info, warn};

mod instancia;
mod notificacao;

/// Quanto esperar para tentar o serviço de novo.
///
/// O ajudante nasce com a sessão e pode chegar antes do serviço; e o serviço pode ser reiniciado
/// por uma atualização. Nos dois casos o certo é esperar e tentar, para sempre — sem o ajudante a
/// cópia simplesmente não atravessa, e ninguém veria por quê.
const RECONECTAR: Duration = Duration::from_secs(2);

/// Quanto esperar entre o aviso de mudança e a leitura.
///
/// O aviso chega quando o programa que copiou **começou** a escrever, e não quando terminou. Ler na
/// hora abre o clipboard enquanto ele ainda o usa — o `OleFlushClipboard` dele falha, e quem vê o
/// erro é o usuário, no Explorer ou no Office, por causa de nós. Medido na bancada: o `Set-Clipboard`
/// do PowerShell acusou *"a operação de Área de Transferência não foi bem-sucedida"* com a leitura
/// imediata.
///
/// Um quarto de segundo é folga para o escritor terminar e ainda imperceptível para quem copia e
/// atravessa.
const ACOMODAR: Duration = Duration::from_millis(250);

/// O que acorda o laço principal.
#[derive(Debug)]
enum Evento {
    /// O sistema avisou que o clipboard mudou.
    Mudou,
    /// O serviço disse algo.
    Aviso(Aviso),
    /// O serviço recusou um pedido: a cópia que ele levava não atravessou.
    Recusado,
    /// A conexão com o serviço caiu.
    Caiu,
}

/// Serve até o processo ser encerrado.
///
/// # Errors
///
/// Só quando não há clipboard nenhum nesta sessão — um *greeter*, um console. Aí não há o que
/// fazer, e sair é mais honesto que girar.
pub(crate) fn servir() -> Result<()> {
    // Vive até o processo sair: é ela que faz um segundo ajudante desistir.
    let _trava = match instancia::ser_o_unico() {
        Ok(Some(trava)) => Some(trava),
        Ok(None) => {
            info!("já há um ajudante de clipboard deste usuário; este sai");
            return Ok(());
        }
        Err(erro) => {
            warn!(%erro, "sem a trava de instância única; seguindo assim mesmo");
            None
        }
    };
    let mut clip = ir_clip::abrir().context("abrindo o clipboard da sessão")?;
    let (eventos, recebe) = mpsc::channel();
    vigiar_em_thread(eventos.clone());

    let mut eco = Eco::nova();
    let mut notificador = notificacao::Notificador::default();
    let endereco = ir_ipc::cliente::endereco_do_controle();
    loop {
        match conectar(&endereco, &eventos) {
            Ok(mut escrita) => {
                info!("ajudante de clipboard ligado ao serviço");
                atender(Partes {
                    eventos: &recebe,
                    escrita: escrita.as_mut(),
                    clip: clip.as_mut(),
                    eco: &mut eco,
                    notificador: &mut notificador,
                });
                warn!("a conexão com o serviço caiu; tentando de novo");
            }
            Err(erro) => debug!(%erro, "o serviço ainda não atende"),
        }
        thread::sleep(RECONECTAR);
    }
}

/// Liga ao serviço, pede para acompanhar, e põe uma thread lendo o que ele disser.
fn conectar(endereco: &str, eventos: &Sender<Evento>) -> Result<Box<dyn Write + Send>> {
    let (mut escrita, leitura) = ir_ipc::cliente::abrir(endereco)
        .with_context(|| format!("abrindo o canal de controle em {endereco}"))?;
    pedir(escrita.as_mut(), &Pedido::AcompanharClipboard)?;
    let eventos = eventos.clone();
    thread::spawn(move || ler_avisos(leitura, &eventos));
    Ok(escrita)
}

/// Traz os avisos do serviço para o laço principal, até a conexão cair.
fn ler_avisos(mut leitura: Box<dyn Read + Send>, eventos: &Sender<Evento>) {
    while let Ok(Some(mensagem)) = crate::ler_quadro::<ParaInterface>(&mut leitura) {
        match mensagem {
            ParaInterface::Aviso(aviso) => {
                if eventos.send(Evento::Aviso(aviso)).is_err() {
                    return;
                }
            }
            ParaInterface::Resposta(Resposta::Falha(falha)) => {
                if falha == Falha::SemPermissao {
                    warn!(
                        "o serviço recusou o ajudante: o usuário precisa estar no grupo `inputremote`"
                    );
                }
                if eventos.send(Evento::Recusado).is_err() {
                    return;
                }
            }
            // `Feito` e o resto não pedem nada daqui.
            _ => {}
        }
    }
    let _ = eventos.send(Evento::Caiu);
}

/// O que o laço de uma conexão precisa. Juntos porque são a mesma coisa: a sessão do usuário.
struct Partes<'a> {
    eventos: &'a Receiver<Evento>,
    escrita: &'a mut dyn Write,
    clip: &'a mut dyn Clipboard,
    eco: &'a mut Eco,
    /// Conta ao usuário o que está acontecendo com a cópia (no Linux, pela notificação do sistema).
    notificador: &'a mut notificacao::Notificador,
}

/// O laço principal de uma conexão.
fn atender(partes: Partes<'_>) {
    let Partes {
        eventos,
        escrita,
        clip,
        eco,
        notificador,
    } = partes;
    while let Ok(evento) = eventos.recv() {
        match evento {
            Evento::Mudou | Evento::Aviso(Aviso::LerClipboard) => oferecer(escrita, clip, eco),
            Evento::Aviso(Aviso::Transferencia(transferencia)) => {
                notificador.contar(&transferencia);
                reagir(&transferencia, clip, eco);
            }
            Evento::Aviso(Aviso::TextoRecebido(texto)) => {
                publicar(&Conteudo::texto(texto.como_str()), clip, eco);
            }
            // Sem par, ou sem permissão: a próxima travessia tem de tentar a mesma cópia de novo.
            Evento::Recusado => eco.oferta_falhou(),
            Evento::Caiu => return,
            Evento::Aviso(_) => {}
        }
    }
}

/// Lê o clipboard e, se ele mudou de verdade, oferece ao par.
fn oferecer(escrita: &mut dyn Write, clip: &mut dyn Clipboard, eco: &mut Eco) {
    let conteudo = match clip.ler() {
        Ok(Some(conteudo)) => conteudo,
        Ok(None) => return,
        Err(erro) => {
            debug!(%erro, "não consegui ler o clipboard");
            return;
        }
    };
    let Some(pedido) = pedido_para(&conteudo) else {
        // Não marcado como oferecido: se um dia houver caminho, a mesma cópia tem de ir.
        debug!(
            tipo = conteudo.tipo().name(),
            bytes = conteudo.tamanho(),
            "este clipboard não atravessa"
        );
        return;
    };
    if !eco.oferecer(&conteudo) {
        return;
    }
    // Tipo e tamanho, nunca o conteúdo nem nomes ([04, §7](../../../docs/04-seguranca.md)).
    info!(
        tipo = conteudo.tipo().name(),
        bytes = conteudo.tamanho(),
        "oferecendo o clipboard ao par"
    );
    if pedir(escrita, &pedido).is_err() {
        eco.oferta_falhou();
    }
}

/// O pedido que leva este conteúdo, se ele tem caminho.
fn pedido_para(conteudo: &Conteudo) -> Option<Pedido> {
    match conteudo {
        Conteudo::Arquivos(caminhos) => Some(Pedido::EnviarArquivos {
            caminhos: caminhos
                .iter()
                .map(|caminho| caminho.to_string_lossy().into_owned())
                .collect(),
        }),
        Conteudo::Texto(texto) => TextoDoClipboard::novo(texto.clone()).map(Pedido::OferecerTexto),
        // Imagem ainda não tem caminho.
        _ => None,
    }
}

/// Reage ao andamento de uma transferência.
fn reagir(transferencia: &Transferencia, clip: &mut dyn Clipboard, eco: &mut Eco) {
    match (&transferencia.sentido, &transferencia.fase) {
        (Sentido::Recebendo, Fase::Concluida { destino }) if !destino.is_empty() => {
            publicar(&Conteudo::Arquivos(vec![PathBuf::from(destino)]), clip, eco);
        }
        // Não chegou do outro lado: a próxima cópia igual tem de ir de novo.
        (Sentido::Enviando, Fase::Parada(_)) => eco.oferta_falhou(),
        _ => {}
    }
}

/// Põe no clipboard o que veio do par.
fn publicar(chegou: &Conteudo, clip: &mut dyn Clipboard, eco: &mut Eco) {
    // Antes de publicar, sempre: o aviso de mudança pode chegar antes de `publicar` voltar.
    eco.publicamos(chegou);
    match clip.publicar(chegou) {
        Ok(()) => info!(
            tipo = chegou.tipo().name(),
            "o que chegou está no clipboard; é só colar"
        ),
        Err(erro) => warn!(%erro, "não consegui pôr no clipboard o que chegou"),
    }
}

/// Espera avisos de mudança numa thread própria, onde o sistema os oferece.
///
/// A thread cria o próprio vigia: no Windows o aviso chega à fila de mensagens da thread que criou
/// a janela, e é por isso que `ir_clip::Vigia` não é `Send`.
fn vigiar_em_thread(eventos: Sender<Evento>) {
    thread::spawn(move || {
        let mut vigia = match ir_clip::vigiar() {
            Ok(vigia) => vigia,
            Err(erro) => {
                // O caso do GNOME, e não um defeito: a travessia é o gatilho aí.
                info!(%erro, "sem aviso de mudança; o clipboard sincroniza na travessia");
                return;
            }
        };
        while vigia.proxima().is_ok() {
            thread::sleep(ACOMODAR);
            if eventos.send(Evento::Mudou).is_err() {
                return;
            }
        }
    });
}

/// Manda um pedido ao serviço.
fn pedir(escrita: &mut dyn Write, pedido: &Pedido) -> Result<()> {
    let bytes = codec::codificar(pedido).context("codificando o pedido")?;
    escrita.write_all(&bytes).context("escrevendo o pedido")?;
    escrita.flush().context("esvaziando o pedido")?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod testes;
