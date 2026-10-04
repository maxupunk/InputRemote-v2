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

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use ir_clip::{Clipboard, Conteudo, Eco};
use ir_ipc::transferencia::{Fase, Sentido, Transferencia};
use ir_ipc::{Aviso, Pedido, TextoDoClipboard};
use ir_recado::Recado;
use tracing::{debug, info, warn};

pub(crate) mod atualizacao;
mod imagem;
pub(crate) mod instancia;
mod notificacao;
mod pasta;
mod rede;
mod servico;

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
    /// Este processo é de antes de uma atualização: o serviço fala uma versão do canal que ele não
    /// entende, ou o executável em disco já é outro ([`atualizacao`]).
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
    let executavel = atualizacao::Executavel::este();
    if let Some(executavel) = executavel.clone() {
        atualizacao::vigiar(executavel, eventos.clone());
    }

    let mut eco = Eco::nova();
    let mut notificador = notificacao::Notificador::default();
    let endereco = ir_ipc::endereco::do_controle();
    loop {
        match servico::conectar(&endereco, &eventos) {
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
                    info!("este ajudante é de antes da atualização e sai, para subir o instalado");
                    return Ok(());
                }
                warn!("a conexão com o serviço caiu; tentando de novo");
            }
            Err(erro) => debug!(%erro, "o serviço ainda não atende"),
        }
        // Desligado do serviço, ninguém lê o aviso da thread que vigia o executável.
        if executavel
            .as_ref()
            .is_some_and(atualizacao::Executavel::mudou)
        {
            info!(
                "o executável do ajudante foi atualizado; este processo sai, para subir o instalado"
            );
            return Ok(());
        }
        thread::sleep(RECONECTAR);
    }
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
    // As pastas compartilhadas daqui: o serviço manda a lista na conexão e a cada mudança.
    let mut pastas = pasta::Pastas::default();
    // Copiar e colar, das Preferências: quem decide é o serviço; aqui só se evita o trabalho de ler
    // e preparar o que não vai sair — trazer uma pasta de rede para perto pode levar minutos.
    let mut ligado = true;
    while let Ok(evento) = eventos.recv() {
        match evento {
            Evento::Mudou | Evento::Aviso(Aviso::LerClipboard) if ligado => {
                oferecer(escrita, clip, eco, &pastas);
            }
            Evento::Aviso(Aviso::EstadoMudou(estado)) => ligado = estado.copiar_e_colar,
            // O outro computador mudou copiar e colar, e este acompanhou: quem está aqui precisa
            // saber por que o Ctrl+C parou (ou voltou) sem ter mexido em nada.
            Evento::Aviso(Aviso::CopiarEColarAjustado { ligado, par }) => {
                notificador.avisar(&Recado::copiar_e_colar_ajustado(ligado, &par));
            }
            Evento::Aviso(Aviso::PastasMudaram(lista)) => pastas.atualizar(&lista),
            // O outro computador copiou da pasta: os mesmos arquivos daqui, no clipboard.
            Evento::Aviso(Aviso::ArquivosDaPasta(caminhos)) => {
                let caminhos = caminhos.into_iter().map(PathBuf::from).collect();
                publicar(&Conteudo::Arquivos(caminhos), clip, eco);
            }
            Evento::Aviso(Aviso::Transferencia(transferencia)) => {
                notificador.contar(&transferencia);
                reagir(&transferencia, clip, eco);
            }
            Evento::Aviso(Aviso::TextoRecebido(texto)) => {
                publicar(&Conteudo::texto(texto.as_str()), clip, eco);
            }
            Evento::Aviso(Aviso::ArquivosChegando(chegando)) => prometer(&chegando, clip),
            // Sem par, ou sem permissão: a próxima travessia tem de tentar a mesma cópia de novo.
            Evento::Recusado => eco.oferta_falhou(),
            Evento::Caiu => return Fim::Caiu,
            Evento::Incompativel => return Fim::Incompativel,
            // Desligado, nem se lê o clipboard; e os avisos que não são daqui.
            Evento::Mudou | Evento::Aviso(_) => {}
        }
    }
    Fim::Caiu
}

/// Lê o clipboard e, se ele mudou de verdade, oferece ao par.
fn oferecer(
    escrita: &mut dyn Write,
    clip: &mut dyn Clipboard,
    eco: &mut Eco,
    pastas: &pasta::Pastas,
) {
    let conteudo = match clip.ler() {
        Ok(Some(conteudo)) => conteudo,
        Ok(None) => return,
        Err(erro) => {
            debug!(%erro, "não consegui ler o clipboard");
            return;
        }
    };
    let Some(pedido) = pastas.pedido(&conteudo, pedido_para) else {
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
    if let Pedido::EnviarArquivos { caminhos } = &pedido {
        pasta::anotar(&caminhos.iter().map(PathBuf::from).collect::<Vec<_>>());
    }
    // Tipo e tamanho, nunca o conteúdo nem nomes ([04, §7](../../../docs/04-seguranca.md)).
    info!(
        tipo = conteudo.tipo().name(),
        bytes = conteudo.tamanho(),
        "oferecendo o clipboard ao par"
    );
    if servico::pedir(escrita, &pedido).is_err() {
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
                pasta::anotar(std::slice::from_ref(&destino));
                publicar(&Conteudo::Arquivos(vec![destino]), clip, eco);
            }
        }
        (Sentido::Recebendo, Fase::Parada(_)) => clip.desfazer_promessa(),
        // Não chegou do outro lado: a próxima cópia igual tem de ir de novo.
        (Sentido::Enviando, Fase::Parada(_)) => eco.oferta_falhou(),
        _ => {}
    }
}

/// Arquivos começaram a chegar: o clipboard já os promete, e colar já cola — cada arquivo é lido
/// à medida que chega. A imagem fica de fora: ela vira imagem no clipboard, e não arquivo.
fn prometer(chegando: &ir_ipc::Chegando, clip: &mut dyn Clipboard) {
    if imagem::e_nome_de_imagem(&chegando.nome) {
        return;
    }
    let chegada = ir_clip::Chegada {
        montagem: PathBuf::from(&chegando.montagem),
        publicada_em: PathBuf::from(&chegando.publicada_em),
        itens: chegando
            .itens
            .iter()
            .map(|item| ir_clip::ItemDaChegada {
                caminho: item.caminho.clone(),
                tamanho: item.tamanho,
                pasta: item.pasta,
            })
            .collect(),
    };
    match clip.prometer_arquivos(&chegada) {
        Ok(()) => debug!(
            itens = chegada.itens.len(),
            "a cópia que chega já está no clipboard"
        ),
        Err(erro) => debug!(%erro, "não consegui prometer a cópia; ela aparece quando chegar"),
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

#[cfg(test)]
mod testes;
