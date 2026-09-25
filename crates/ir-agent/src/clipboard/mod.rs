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
//! Aviso::LerClipboard (travessia)    ┴─► lê ─► Eco::oferecer ─┼► Pedido::OferecerTexto
//!                                                             └► imagem: PNG em arquivo, e envio
//!
//! Aviso::Transferencia(recebendo, concluída) ┐
//! Aviso::TextoRecebido                       ┴─► Eco::publicamos ─► publica o que chegou
//! ```
//!
//! Nada de política aqui: quando ler, quem decide é o serviço; o que é eco, quem decide é o [`Eco`].
//! Arquivos vão pelo canal de dados (TCP); texto, pelo canal 4 da sessão, em qualquer portador.
//! Imagem vai como arquivo PNG de nome reconhecível, e do outro lado volta a ser imagem.
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

mod imagem;
mod instancia;
mod notificacao;
mod rede;

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
    /// O serviço fala uma versão do canal que este binário não entende: ele foi atualizado, e este
    /// processo é de antes.
    Incompativel,
}

/// Por que o laço de uma conexão terminou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fim {
    /// A conexão caiu: tentar de novo.
    Caiu,
    /// Este processo é mais velho que o serviço: sair.
    Incompativel,
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
    let endereco = ir_ipc::endereco::do_controle();
    loop {
        match conectar(&endereco, &eventos) {
            Ok(mut escrita) => {
                info!("ajudante de clipboard ligado ao serviço");
                let fim = atender(Partes {
                    eventos: &recebe,
                    escrita: escrita.as_mut(),
                    clip: clip.as_mut(),
                    eco: &mut eco,
                    notificador: &mut notificador,
                });
                if fim == Fim::Incompativel {
                    // Reconectar não adianta: cada quadro viria inválido, e com a trava de
                    // instância única este processo impediria o novo de subir. Saindo, quem zela
                    // pelo ajudante lança o binário instalado. Antes disto, depois de uma
                    // atualização o texto não atravessava mais até sair da sessão.
                    info!(
                        "o serviço fala outra versão do canal: este ajudante é de antes da atualização e sai, para subir o instalado"
                    );
                    return Ok(());
                }
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
///
/// O motivo de parar é registrado. Sem ele, "a conexão caiu" cobria três coisas muito diferentes —
/// o serviço fechou, o quadro veio inválido, o canal deu erro — e a cópia que não atravessava não
/// tinha como ser explicada. Custou uma investigação inteira.
fn ler_avisos(mut leitura: Box<dyn Read + Send>, eventos: &Sender<Evento>) {
    loop {
        let mensagem = match crate::ler_quadro::<ParaInterface>(&mut leitura) {
            Ok(Some(mensagem)) => mensagem,
            Ok(None) => {
                debug!("o serviço fechou o canal de controle");
                break;
            }
            Err(erro) if e_incompativel(&erro) => {
                warn!(erro = ?erro, "o serviço mandou um quadro que este ajudante não entende");
                let _ = eventos.send(Evento::Incompativel);
                return;
            }
            Err(erro) => {
                warn!(erro = ?erro, "falha ao ler do canal de controle");
                break;
            }
        };
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

/// Se o erro de leitura é de formato, e não de canal: um quadro inteiro que não decodifica.
///
/// Os dois lados são do mesmo pacote, então isto só acontece com um processo que sobreviveu a uma
/// atualização do serviço.
fn e_incompativel(erro: &anyhow::Error) -> bool {
    erro.chain().any(|causa| {
        causa
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::InvalidData)
    })
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

/// O laço principal de uma conexão, até ela terminar.
fn atender(partes: Partes<'_>) -> Fim {
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
                publicar(&Conteudo::texto(texto.as_str()), clip, eco);
            }
            // Sem par, ou sem permissão: a próxima travessia tem de tentar a mesma cópia de novo.
            Evento::Recusado => eco.oferta_falhou(),
            Evento::Caiu => return Fim::Caiu,
            Evento::Incompativel => return Fim::Incompativel,
            Evento::Aviso(_) => {}
        }
    }
    Fim::Caiu
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
    // Só depois da guarda de eco: trazer uma pasta de rede para perto pode ser demorado, e não pode
    // se repetir a cada aviso de mudança do mesmo clipboard.
    let pedido = trazer_da_rede(pedido);
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

/// Troca o que está numa pasta de rede por uma cópia local, que o serviço consegue ler.
fn trazer_da_rede(pedido: Pedido) -> Pedido {
    match pedido {
        Pedido::EnviarArquivos { caminhos } => {
            let originais: Vec<PathBuf> = caminhos.iter().map(PathBuf::from).collect();
            let perto = rede::trazer_para_perto(&originais, &imagem::pasta_temporaria());
            Pedido::EnviarArquivos {
                caminhos: perto
                    .iter()
                    .map(|caminho| caminho.to_string_lossy().into_owned())
                    .collect(),
            }
        }
        outro => outro,
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
        Conteudo::Texto(texto) => TextoDoClipboard::new(texto.clone()).map(Pedido::OferecerTexto),
        // A imagem vai como arquivo PNG, pelo canal de dados; o outro lado a reconhece pelo nome.
        Conteudo::Imagem(png) => {
            match imagem::gravar_para_enviar(png, &imagem::pasta_temporaria()) {
                Ok(caminho) => Some(Pedido::EnviarArquivos {
                    caminhos: vec![caminho.to_string_lossy().into_owned()],
                }),
                Err(erro) => {
                    warn!(%erro, "não consegui guardar a imagem copiada para enviar");
                    None
                }
            }
        }
        _ => None,
    }
}

/// Reage ao andamento de uma transferência.
fn reagir(transferencia: &Transferencia, clip: &mut dyn Clipboard, eco: &mut Eco) {
    match (&transferencia.sentido, &transferencia.fase) {
        (Sentido::Recebendo, Fase::Concluida { destino }) if !destino.is_empty() => {
            let destino = PathBuf::from(destino);
            if imagem::e_imagem_do_clipboard(&destino) {
                match imagem::ler_recebida(&destino) {
                    Ok(chegou) => publicar(&chegou, clip, eco),
                    Err(erro) => warn!(%erro, "não consegui ler a imagem que chegou"),
                }
            } else {
                publicar(&Conteudo::Arquivos(vec![destino]), clip, eco);
            }
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
    codec::escrever_em(escrita, pedido).context("escrevendo o pedido")
}

#[cfg(test)]
mod testes;
