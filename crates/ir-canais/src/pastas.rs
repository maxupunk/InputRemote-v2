//! O canal do ajudante das pastas, do lado do serviço
//! ([ADR-0015](../../../docs/adr/0015-pastas-compartilhadas.md), §5).
//!
//! O serviço não abre nenhuma mensagem da pasta: ele **repassa**. O que vem do par vai ao ajudante
//! do usuário dono daquela pasta; o que o ajudante manda vai ao par, pela faixa do canal de arquivos
//! ([`ir_transferencia::Faixa`]). O ator não está no caminho — bloco de arquivo não espera o
//! compasso de 5 ms da entrada.
//!
//! # De quem é cada pasta
//!
//! A primeira conexão que fala de uma pasta fica dona dela, pela identidade do usuário — o `uid` no
//! Linux, o SID no Windows. Outro usuário que a reivindique depois é ignorado: duas sessões abertas
//! na mesma máquina não leem a pasta uma da outra. Mensagem do par sobre pasta sem dono — a oferta
//! de uma pasta nova — vai a todos os ajudantes, e quem aceitar fica dono.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use ir_ipc::Aviso;
use ir_ipc::pastas::{
    ComandoDePasta, DoAjudanteDePastas, IdDePasta, MensagemDoPar, ParaOAjudanteDePastas,
    ResumoDePasta,
};
use ir_transferencia::{EstadoDaFaixa, Faixa};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

use super::escuta::{Chamada, Conexao, Escuta};
use super::quadros;

/// Uma pasta pelo identificador cru — o mesmo do protocolo e do vocabulário da janela.
type Chave = [u8; 16];

/// Por onde o ator fala com os ajudantes das pastas.
#[derive(Debug, Clone)]
pub struct Pastas {
    hub: Arc<Mutex<Hub>>,
    ajudantes: ir_sessao::Ajudantes,
}

#[derive(Debug, Default)]
struct Hub {
    ligadas: BTreeMap<u64, Ligada>,
    donos: BTreeMap<Chave, String>,
    proxima: u64,
    estado: EstadoDaFaixa,
}

#[derive(Debug)]
struct Ligada {
    quem: String,
    saida: mpsc::UnboundedSender<ParaOAjudanteDePastas>,
    resumo: Vec<ResumoDePasta>,
}

/// O endereço do canal do ajudante das pastas, sobrescrevível por `IR_PASTAS_ENDPOINT`.
#[must_use]
pub fn endereco_das_pastas() -> String {
    ir_ipc::endereco::das_pastas()
}

/// Sobe o canal do ajudante das pastas, ligado à faixa do canal de arquivos.
///
/// # Errors
///
/// Erro do sistema ao abrir o ponto de escuta.
pub fn iniciar_pastas(faixa: Faixa, avisos: broadcast::Sender<Aviso>) -> Result<Pastas> {
    let escuta = Escuta::abrir(
        &endereco_das_pastas(),
        super::escuta::Acesso::UsuarioInterativo,
    )?;
    let pastas = Pastas {
        hub: Arc::default(),
        ajudantes: ir_sessao::Ajudantes::default(),
    };
    let Faixa {
        para_o_par,
        do_par,
        estado,
        copias,
    } = faixa;
    let avisar = Avisar { avisos, copias };
    tokio::spawn(aceitar(escuta, pastas.clone(), para_o_par.clone(), avisar));
    tokio::spawn(do_par_aos_ajudantes(pastas.clone(), do_par, para_o_par));
    tokio::spawn(contar_o_enlace(pastas.clone(), estado));
    Ok(pastas)
}

impl Pastas {
    /// Quantos ajudantes das pastas estão ligados — o que o zelador do Windows confere.
    #[must_use]
    pub fn ajudantes(&self) -> ir_sessao::Ajudantes {
        self.ajudantes.clone()
    }

    /// Repassa um pedido da janela ao ajudante. `false` quando não há ajudante para recebê-lo.
    ///
    /// Vai ao ajudante do mesmo usuário, quando se sabe quem pediu; senão, ao que conectou por
    /// último — o da sessão que está na frente.
    #[must_use]
    pub fn comando(&self, comando: ComandoDePasta, quem: Option<&str>) -> bool {
        let Ok(hub) = self.hub.lock() else {
            return false;
        };
        let alvo = quem
            .and_then(|quem| hub.ligadas.values().rev().find(|l| l.quem == quem))
            .or_else(|| hub.ligadas.values().next_back());
        alvo.is_some_and(|ligada| {
            ligada
                .saida
                .send(ParaOAjudanteDePastas::Comando(comando))
                .is_ok()
        })
    }

    /// As pastas de todos os ajudantes ligados, para a janela.
    #[must_use]
    pub fn resumo(&self) -> Vec<ResumoDePasta> {
        self.hub.lock().map_or_else(
            |_| Vec::new(),
            |hub| {
                hub.ligadas
                    .values()
                    .flat_map(|l| l.resumo.iter().cloned())
                    .collect()
            },
        )
    }

    fn ligar(&self, quem: String) -> (u64, mpsc::UnboundedReceiver<ParaOAjudanteDePastas>) {
        let (saida, entrada) = mpsc::unbounded_channel();
        let mut hub = self
            .hub
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = hub.proxima;
        hub.proxima += 1;
        let estado = hub.estado.clone();
        let _ = saida.send(enlace(&estado));
        hub.ligadas.insert(
            id,
            Ligada {
                quem,
                saida,
                resumo: Vec::new(),
            },
        );
        (id, entrada)
    }

    fn desligar(&self, id: u64) {
        if let Ok(mut hub) = self.hub.lock() {
            hub.ligadas.remove(&id);
        }
    }

    /// Se esta conexão pode falar desta pasta; a primeira que falar fica dona.
    fn pode(&self, id: u64, pasta: Chave) -> bool {
        let Ok(mut hub) = self.hub.lock() else {
            return false;
        };
        let Some(quem) = hub.ligadas.get(&id).map(|l| l.quem.clone()) else {
            return false;
        };
        let dono = hub.donos.entry(pasta).or_insert_with(|| quem.clone());
        *dono == quem
    }

    fn guardar_resumo(&self, id: u64, resumo: Vec<ResumoDePasta>) {
        if let Ok(mut hub) = self.hub.lock()
            && let Some(ligada) = hub.ligadas.get_mut(&id)
        {
            ligada.resumo = resumo;
        }
    }

    /// Entrega uma mensagem do par: ao dono da pasta, ou a todos quando ela não tem dono ou fala de
    /// todas. Devolve quantos a receberam.
    fn entregar(&self, mensagem: &MensagemDoPar) -> usize {
        let Ok(hub) = self.hub.lock() else {
            return 0;
        };
        let dono = mensagem.folder().and_then(|p| hub.donos.get(&p.0));
        let mut entregues = 0;
        for ligada in hub.ligadas.values() {
            if dono.is_some_and(|dono| *dono != ligada.quem) {
                continue;
            }
            if ligada
                .saida
                .send(ParaOAjudanteDePastas::DoPar(mensagem.clone()))
                .is_ok()
            {
                entregues += 1;
            }
        }
        entregues
    }
}

fn enlace(estado: &EstadoDaFaixa) -> ParaOAjudanteDePastas {
    ParaOAjudanteDePastas::Enlace {
        // Sem saber ainda a versão do par, o canal conta como fora: o ajudante espera, e a tela diz
        // "não está ao alcance" — e não "atualize o outro computador", que seria um palpite.
        de_pe: estado.de_pe && estado.versao_conhecida,
        par_suporta: estado.par_suporta,
        nome_do_par: estado.nome_do_par.clone(),
    }
}

/// Conta a cada ajudante quando o canal sobe, cai, ou o par muda.
async fn contar_o_enlace(pastas: Pastas, mut estado: tokio::sync::watch::Receiver<EstadoDaFaixa>) {
    loop {
        let atual = estado.borrow_and_update().clone();
        if let Ok(mut hub) = pastas.hub.lock() {
            hub.estado = atual.clone();
            for ligada in hub.ligadas.values() {
                let _ = ligada.saida.send(enlace(&atual));
            }
        }
        if estado.changed().await.is_err() {
            return;
        }
    }
}

/// O que vem do par vai a quem é dono. Um `Hello` sem ninguém para ouvir é respondido daqui: o par
/// fica sabendo que as pastas deste lado estão fora, e não espera.
async fn do_par_aos_ajudantes(
    pastas: Pastas,
    mut do_par: mpsc::Receiver<MensagemDoPar>,
    para_o_par: mpsc::Sender<MensagemDoPar>,
) {
    while let Some(mensagem) = do_par.recv().await {
        let ola = mensagem.folder().is_none() && mensagem != MensagemDoPar::HelperAbsent;
        if pastas.entregar(&mensagem) == 0 && ola {
            debug!("pasta: nenhum ajudante ligado; o par fica sabendo");
            let _ = para_o_par.send(MensagemDoPar::HelperAbsent).await;
        }
    }
}

/// Aceita ajudantes para sempre, uma tarefa por conexão.
async fn aceitar(
    mut escuta: Escuta,
    pastas: Pastas,
    para_o_par: mpsc::Sender<MensagemDoPar>,
    avisar: Avisar,
) {
    loop {
        match escuta.aceitar().await {
            Ok((conexao, Chamada::Permitida)) => {
                let quem = escuta.quem_conectou(&conexao);
                let atendimento = Atendimento {
                    pastas: pastas.clone(),
                    para_o_par: para_o_par.clone(),
                    avisar: avisar.clone(),
                };
                tokio::spawn(atendimento.atender(conexao, quem));
            }
            Ok((_, Chamada::Negada { uid })) => {
                warn!(
                    uid,
                    "ajudante das pastas recusado: o usuário não está no grupo `inputremote`"
                );
            }
            Err(erro) => {
                warn!(%erro, "falha ao aceitar o ajudante das pastas");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
}

struct Atendimento {
    pastas: Pastas,
    para_o_par: mpsc::Sender<MensagemDoPar>,
    avisar: Avisar,
}

/// Para onde vão os avisos do canal das pastas, e a chave de copiar e colar que decide se o que o
/// outro computador copiou de uma pasta chega ao clipboard daqui.
#[derive(Clone)]
struct Avisar {
    avisos: broadcast::Sender<Aviso>,
    copias: ir_transferencia::ChaveDaCopia,
}

impl Atendimento {
    /// Atende um ajudante até ele desligar.
    async fn atender(self, mut conexao: Conexao, quem: Option<String>) {
        // O primeiro quadro é lido antes de dividir a conexão: no Windows, só depois de ler algo do
        // pipe o sistema diz quem está do outro lado.
        let primeiro = match quadros::ler::<_, DoAjudanteDePastas>(&mut conexao).await {
            Ok(Some(primeiro)) => primeiro,
            Ok(None) => return,
            Err(erro) => {
                warn!(%erro, "quadro malformado do ajudante das pastas");
                return;
            }
        };
        let Some(quem) = quem.or_else(|| super::escuta::quem_depois_de_ler(&conexao)) else {
            warn!("não consegui identificar o ajudante das pastas; conexão recusada");
            return;
        };
        info!("ajudante das pastas conectado");
        let _presenca = self.pastas.ajudantes.entrou();
        let (id, mut para_o_ajudante) = self.pastas.ligar(quem);
        let (mut leitura, mut escrita) = tokio::io::split(conexao);
        self.tratar(id, primeiro).await;
        loop {
            tokio::select! {
                lido = quadros::ler::<_, DoAjudanteDePastas>(&mut leitura) => match lido {
                    Ok(Some(mensagem)) => self.tratar(id, mensagem).await,
                    Ok(None) => break,
                    Err(erro) => {
                        warn!(%erro, "quadro malformado do ajudante das pastas");
                        break;
                    }
                },
                Some(saida) = para_o_ajudante.recv() => {
                    if quadros::escrever(&mut escrita, &saida).await.is_err() {
                        break;
                    }
                }
            }
        }
        self.pastas.desligar(id);
        let _ = self
            .avisar
            .avisos
            .send(Aviso::PastasMudaram(self.pastas.resumo()));
        info!("ajudante das pastas desconectado");
    }

    async fn tratar(&self, id: u64, mensagem: DoAjudanteDePastas) {
        match mensagem {
            DoAjudanteDePastas::Apresentar { pastas } => {
                for IdDePasta(pasta) in pastas {
                    if !self.pastas.pode(id, pasta) {
                        warn!("pasta de outro usuário reivindicada; ignorada");
                    }
                }
            }
            DoAjudanteDePastas::ParaOPar(mensagem) => {
                let permitida = mensagem.folder().is_none_or(|p| self.pastas.pode(id, p.0));
                if permitida {
                    let _ = self.para_o_par.send(mensagem).await;
                } else {
                    warn!("ajudante falou de pasta de outro usuário; mensagem descartada");
                }
            }
            DoAjudanteDePastas::Resumo(resumo) => {
                self.pastas.guardar_resumo(id, resumo);
                let _ = self
                    .avisar
                    .avisos
                    .send(Aviso::PastasMudaram(self.pastas.resumo()));
            }
            DoAjudanteDePastas::Recado(frase) => {
                let _ = self.avisar.avisos.send(Aviso::RecadoDasPastas(frase));
            }
            // O outro computador copiou arquivos da pasta: os caminhos daqui vão ao clipboard.
            DoAjudanteDePastas::PorNoClipboard(caminhos) if self.avisar.copias.ligada() => {
                let _ = self.avisar.avisos.send(Aviso::ArquivosDaPasta(caminhos));
            }
            _ => {}
        }
    }
}
