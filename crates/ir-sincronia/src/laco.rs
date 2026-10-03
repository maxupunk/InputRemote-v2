//! O laço do ajudante das pastas: ligar ao serviço, ouvir, varrer o que mudou, andar.
//!
//! Uma thread só decide; as outras — a que lê o canal local, a vigia do disco, os *callbacks* do
//! Windows para os arquivos sob demanda — mandam o que viram por um canal de eventos só. Com uma
//! fila só, a ordem do que acontece é a ordem em que se trata, e nada precisa de trava.
//!
//! O canal de eventos vive o processo inteiro, e não uma conexão com o serviço: o Windows pode
//! pedir o conteúdo de um arquivo a qualquer hora, e um pedido sem resposta deixa o programa que o
//! abriu esperando um minuto. Fora do serviço o pedido é recusado na hora. Os eventos do canal
//! local levam o número da conexão: um "caiu" atrasado de uma conexão antiga não derruba a nova.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ir_ipc::codec;
use ir_ipc::pastas::{DoAjudanteDePastas, ParaOAjudanteDePastas, ResumoDePasta};
use ir_proto::message::{FolderId, FolderMessage};
use tracing::{debug, info, warn};

use crate::lugar::Lugar;
use crate::pastas::Pastas;
use crate::vigia::Vigia;

/// O que chega ao laço.
#[derive(Debug)]
pub enum Evento {
    /// O serviço disse algo, nesta conexão.
    DoServico(u64, ParaOAjudanteDePastas),
    /// Algo mudou neste caminho do disco.
    Mudou(PathBuf),
    /// A vigia perdeu avisos: varrer tudo.
    VarrerTudo,
    /// Esta conexão com o serviço caiu.
    Caiu(u64),
    /// O serviço mandou o que este processo não entende: ele foi atualizado, e este é o antigo.
    Incompativel(u64),
    /// O Windows pediu algo de um arquivo sob demanda.
    Nuvem(ir_nuvem::Pedido),
}

/// Quanto silêncio esperar depois de uma mudança antes de varrer.
const SILENCIO: Duration = Duration::from_millis(1_500);

/// De quanto em quanto tempo varrer tudo, mesmo sem aviso.
const VARREDURA_GERAL: Duration = Duration::from_secs(300);

/// Quanto esperar para tentar o serviço de novo.
const ESPERA_PELO_SERVICO: Duration = Duration::from_secs(3);

/// Desliga do Windows as pastas sob demanda deste usuário — o passo da desinstalação. Devolve quantas.
///
/// # Errors
///
/// Quando este usuário não tem onde guardar as pastas.
pub fn desregistrar() -> Result<usize> {
    let mut pastas = Pastas::abrir(Lugar::deste_usuario()?);
    Ok(pastas.desligar_sob_demanda())
}

/// Roda o ajudante das pastas enquanto `continuar` disser que sim.
///
/// # Errors
///
/// Quando este usuário não tem onde guardar as pastas.
pub fn rodar(continuar: &dyn Fn() -> bool) -> Result<()> {
    let lugar = Lugar::deste_usuario()?;
    let mut pastas = Pastas::abrir(lugar);
    let (eventos, recebe) = mpsc::channel();
    let para_o_laco = Mutex::new(eventos.clone());
    pastas.usar_nuvem(Arc::new(move |pedido| {
        if let Ok(eventos) = para_o_laco.lock() {
            let _ = eventos.send(Evento::Nuvem(pedido));
        }
    }));
    let vigia = Vigia::nova(eventos.clone())
        .inspect_err(|erro| warn!(%erro, "sem vigia do disco; só a varredura periódica"))
        .ok();
    let mut laco = Laco {
        pastas,
        recebe,
        vigia,
        estado: Estado::default(),
    };
    let endereco = ir_ipc::endereco::das_pastas();
    let mut numero = 0u64;
    while continuar() {
        match ir_ipc::cliente::abrir(&endereco) {
            Ok((escrita, leitura)) => {
                numero += 1;
                info!("ajudante das pastas ligado ao serviço");
                let leitor = eventos.clone();
                std::thread::spawn(move || ler_servico(leitura, &leitor, numero));
                match laco.sessao(numero, escrita, continuar) {
                    Ok(Fim::Reconectar) => {}
                    Ok(Fim::Sair) => return Ok(()),
                    Err(erro) => warn!(erro = format!("{erro:#}"), "a conversa com o serviço caiu"),
                }
            }
            Err(erro) => debug!(%erro, "serviço fora; tento de novo"),
        }
        laco.fora_do_servico(ESPERA_PELO_SERVICO);
    }
    Ok(())
}

enum Fim {
    Reconectar,
    Sair,
}

struct Laco {
    pastas: Pastas,
    recebe: Receiver<Evento>,
    vigia: Option<Vigia>,
    estado: Estado,
}

impl Laco {
    /// Uma conexão com o serviço, até cair.
    fn sessao(
        &mut self,
        numero: u64,
        mut escrita: Box<dyn Write + Send>,
        continuar: &dyn Fn() -> bool,
    ) -> Result<Fim> {
        let ids = self.pastas.ids();
        escrever(
            &mut escrita,
            &DoAjudanteDePastas::Apresentar { pastas: ids },
        )?;
        self.estado.ultimo_resumo = None;
        self.pastas.varrer(None, &mut self.estado.fila);
        loop {
            if let Some(vigia) = self.vigia.as_mut() {
                vigia.acompanhar(&self.pastas.raizes());
            }
            if !continuar() {
                return Ok(Fim::Sair);
            }
            match self.recebe.recv_timeout(Duration::from_millis(200)) {
                Ok(Evento::Caiu(de)) if de == numero => {
                    self.pastas
                        .enlace(false, false, String::new(), &mut self.estado.fila);
                    return Ok(Fim::Reconectar);
                }
                Ok(Evento::Incompativel(de)) if de == numero => {
                    info!("o serviço foi atualizado; este ajudante sai para o novo subir");
                    return Ok(Fim::Sair);
                }
                Ok(Evento::DoServico(de, mensagem)) if de == numero => {
                    self.estado
                        .do_servico(mensagem, &mut self.pastas, &mut escrita)?;
                }
                Ok(evento) => self.estado.local(evento, &mut self.pastas),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(Fim::Sair),
            }
            self.estado.varrer_o_que_venceu(&mut self.pastas);
            self.pastas.andar(&mut self.estado.fila);
            self.estado.despejar(&self.pastas, &mut escrita)?;
        }
    }

    /// Sem o serviço, por um tempo: o disco continua sendo varrido — a fila offline cresce — e o
    /// Windows ouve "não" na hora.
    fn fora_do_servico(&mut self, por: Duration) {
        let ate = Instant::now() + por;
        while let Some(falta) = ate.checked_duration_since(Instant::now()) {
            match self.recebe.recv_timeout(falta) {
                Ok(evento) => self.estado.local(evento, &mut self.pastas),
                Err(_) => break,
            }
        }
        self.estado.varrer_o_que_venceu(&mut self.pastas);
        self.pastas.andar(&mut self.estado.fila);
        // Sem canal, o que era para o par não tem para onde ir; o `Hello` da volta acerta tudo.
        self.estado.fila.clear();
    }
}

#[derive(Default)]
struct Estado {
    /// O que sai para o par, na ordem.
    fila: Vec<FolderMessage>,
    /// As pastas que mudaram, e quando varrê-las.
    sujas: BTreeMap<FolderId, Instant>,
    ultima_geral: Option<Instant>,
    ultimo_resumo: Option<Vec<ResumoDePasta>>,
}

impl Estado {
    fn do_servico(
        &mut self,
        mensagem: ParaOAjudanteDePastas,
        pastas: &mut Pastas,
        escrita: &mut dyn Write,
    ) -> Result<()> {
        match mensagem {
            ParaOAjudanteDePastas::Enlace {
                de_pe,
                par_suporta,
                nome_do_par,
            } => pastas.enlace(de_pe, par_suporta, nome_do_par, &mut self.fila),
            ParaOAjudanteDePastas::DoPar(mensagem) => pastas.do_par(mensagem, &mut self.fila),
            ParaOAjudanteDePastas::Comando(comando) => {
                if let Err(frase) = pastas.comando(comando, &mut self.fila) {
                    escrever(escrita, &DoAjudanteDePastas::Recado(frase))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// O que não vem do serviço: o disco e o Windows. Eventos de uma conexão antiga são ignorados.
    fn local(&mut self, evento: Evento, pastas: &mut Pastas) {
        match evento {
            Evento::Mudou(caminho) => {
                let raizes = pastas.raizes();
                if let Some((pasta, _)) = raizes.iter().find(|(_, r)| caminho.starts_with(r)) {
                    self.sujas.insert(*pasta, Instant::now() + SILENCIO);
                }
            }
            Evento::VarrerTudo => self.ultima_geral = None,
            Evento::Nuvem(pedido) => pastas.da_nuvem(pedido, &mut self.fila),
            _ => {}
        }
    }

    fn varrer_o_que_venceu(&mut self, pastas: &mut Pastas) {
        let agora = Instant::now();
        if self
            .ultima_geral
            .is_none_or(|ultima| agora - ultima > VARREDURA_GERAL)
        {
            pastas.varrer(None, &mut self.fila);
            self.ultima_geral = Some(agora);
            self.sujas.clear();
            return;
        }
        let vencidas: Vec<FolderId> = self
            .sujas
            .iter()
            .filter(|(_, prazo)| **prazo <= agora)
            .map(|(pasta, _)| *pasta)
            .collect();
        for pasta in vencidas {
            self.sujas.remove(&pasta);
            pastas.varrer(Some(pasta), &mut self.fila);
        }
    }

    fn despejar(&mut self, pastas: &Pastas, escrita: &mut dyn Write) -> Result<()> {
        for mensagem in self.fila.drain(..) {
            escrever(escrita, &DoAjudanteDePastas::ParaOPar(mensagem))?;
        }
        let resumo = pastas.resumo();
        if self.ultimo_resumo.as_ref() != Some(&resumo) {
            escrever(escrita, &DoAjudanteDePastas::Resumo(resumo.clone()))?;
            self.ultimo_resumo = Some(resumo);
        }
        Ok(())
    }
}

fn escrever(escrita: &mut dyn Write, mensagem: &DoAjudanteDePastas) -> Result<()> {
    codec::escrever_em(escrita, mensagem).context("escrevendo ao serviço")
}

/// Traz o que o serviço disser nesta conexão, até ela cair.
fn ler_servico(mut leitura: Box<dyn Read + Send>, eventos: &Sender<Evento>, numero: u64) {
    loop {
        match codec::ler_de::<ParaOAjudanteDePastas, _>(&mut leitura) {
            Ok(Some(mensagem)) => {
                if eventos.send(Evento::DoServico(numero, mensagem)).is_err() {
                    return;
                }
            }
            Ok(None) => break,
            Err(erro) if erro.kind() == std::io::ErrorKind::InvalidData => {
                let _ = eventos.send(Evento::Incompativel(numero));
                return;
            }
            Err(erro) => {
                debug!(%erro, "a leitura do canal das pastas caiu");
                break;
            }
        }
    }
    let _ = eventos.send(Evento::Caiu(numero));
}
