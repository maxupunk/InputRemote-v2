//! Por onde o ator do serviço pede cópias, e o que ele sabe do canal de arquivos.
//!
//! A alça, e não o canal: o ator gira no compasso de 5 ms da entrada, e nada aqui espera. Pedir
//! põe na fila e acorda a tarefa que conduz o canal ([`crate::iniciar`]); o que acontece depois
//! chega à interface pelos avisos.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ir_ipc::Aviso;
use tokio::sync::{broadcast, mpsc, watch};
use tracing::debug;

use crate::{Destino, Leitor, faxina, fila, retomada};

/// Por onde o ator pede um envio.
#[derive(Debug, Clone)]
pub struct Pedidos {
    /// A fila de um: a mesma cópia não vai duas vezes, e outra cópia substitui a que está indo.
    pub(crate) fila: Arc<fila::Fila>,
    /// Quem cuida da pasta de recebidos.
    pub(crate) faxineiro: Arc<faxina::Faxineiro>,
    /// O toque que acorda a tarefa de envio. Fechá-lo é como o serviço diz que está saindo.
    pub(crate) acorda: mpsc::UnboundedSender<()>,
    pub(crate) destino: Arc<watch::Sender<Destino>>,
    /// Se há enlace de dados de pé agora. Sem ele, uma cópia pedida aparece na tela esperando a
    /// conexão, em vez de esperar calada.
    pub(crate) de_pe: Arc<AtomicBool>,
    /// Para contar à interface o que houve com uma cópia que esperava e saiu da fila.
    pub(crate) avisos: broadcast::Sender<Aviso>,
    /// A faixa da pasta compartilhada no canal.
    pub(crate) desvio: crate::desvio::Desvio,
    /// O lado do ajudante da faixa, até alguém tomá-lo ([`Self::tomar_faixa`]).
    pub(crate) faixa: Arc<std::sync::Mutex<Option<crate::Faixa>>>,
    /// O que o canal moveu, de cópias e pastas.
    pub(crate) trafego: Arc<ir_transporte::dados::Contador>,
}

impl Pedidos {
    /// Uma alça que não leva a lugar nenhum.
    ///
    /// Para bancada de teste do serviço, que exercita o ator sem canal de arquivos. Não finge
    /// sucesso: [`Self::enviar`] devolve `false`, e o serviço responde ao pedido com falha — que é
    /// a verdade naquele cenário.
    #[must_use]
    pub fn desligada() -> Self {
        let (acorda, _) = mpsc::unbounded_channel();
        let (destino, _) = watch::channel(Destino::default());
        let (avisos, _) = broadcast::channel(1);
        Self {
            fila: Arc::new(fila::Fila::default()),
            faxineiro: faxina::Faxineiro::novo(PathBuf::new(), faxina::Limites::default()),
            acorda,
            destino: Arc::new(destino),
            de_pe: Arc::new(AtomicBool::new(false)),
            avisos,
            desvio: crate::desvio::Desvio::solto(),
            faixa: Arc::default(),
            trafego: Arc::default(),
        }
    }

    /// Quanto o canal de dados moveu até agora, nos dois sentidos.
    #[must_use]
    pub fn trafego(&self) -> ir_ipc::Trafego {
        ir_ipc::Trafego {
            enviados: self.trafego.enviados(),
            recebidos: self.trafego.recebidos(),
        }
    }

    /// O lado do ajudante da pasta compartilhada: por onde mandar ao par e receber dele. Uma vez só —
    /// é de quem serve o canal local das pastas.
    #[must_use]
    pub fn tomar_faixa(&self) -> Option<crate::Faixa> {
        self.faixa.lock().ok().and_then(|mut faixa| faixa.take())
    }

    /// O que a sessão de entrada sabe do par: se a versão acordada conhece pastas, e o nome dele.
    ///
    /// Chamado a cada batida do ator; só muda alguma coisa quando muda de verdade.
    pub fn informar_par(&self, versao: Option<ir_proto::version::ProtocolVersion>, nome: &str) {
        let suporta = versao.is_some_and(ir_proto::version::supports_folders);
        self.desvio.par(suporta, nome);
    }

    /// Esvazia a pasta de recebidos e conta o estado novo quando terminar.
    ///
    /// Apagar gigabytes toca disco e pode demorar; quem pede é o ator do serviço, que gira a cada
    /// 5 ms e não pode esperar. Então isto manda fazer e volta na hora: o tamanho de volta chega
    /// pelo aviso, que é como toda mudança de estado chega à janela.
    pub fn limpar_recebidos(&self, avisos: &broadcast::Sender<Aviso>, estado: ir_ipc::Estado) {
        let faxineiro = Arc::clone(&self.faxineiro);
        let avisos = avisos.clone();
        tokio::spawn(async move {
            faxineiro.esvaziar().await;
            let _ = avisos.send(Aviso::EstadoMudou(ir_ipc::Estado {
                recebidos_bytes: faxineiro.espaco(),
                ..estado
            }));
        });
    }

    /// Quem cuida da pasta de recebidos: quanto ela ocupa, e o pedido de esvaziá-la.
    ///
    /// A janela mostra o tamanho e oferece o botão; o serviço não decide por conta própria apagar
    /// o que o usuário ainda não colou — fora dos limites de [`faxina::Limites`], que são a parte
    /// automática.
    #[must_use]
    pub fn recebidos(&self) -> &Arc<faxina::Faxineiro> {
        &self.faxineiro
    }

    /// O par mudou: pareou-se um, esqueceu-se o que havia, ou ele foi achado em outro endereço.
    ///
    /// O canal em curso, se era com outro par, cai; o novo sobe sozinho.
    pub fn trocar_destino(&self, destino: Destino) {
        self.destino.send_if_modified(|atual| {
            let mudou = *atual != destino;
            *atual = destino;
            mudou
        });
    }

    /// Para a cópia em curso e esquece a que esperava. `false` se não havia cópia.
    ///
    /// O outro lado recebe o cancelamento e apaga o que já gravou: a montagem dele só vira arquivo
    /// no fim. A cópia que esperava a conexão na tela diz aqui mesmo que parou — não há enlace
    /// conduzindo-a para dizer.
    #[must_use]
    pub fn cancelar(&self) -> bool {
        let (havia, a_vista) = self.fila.cancelar_tudo();
        if let Some(trabalho) = a_vista {
            retomada::espera_cancelada(&self.avisos, &trabalho);
        }
        havia
    }

    /// Pede o envio destes caminhos, lidos com a autoridade de `leitor`. `false` quando a
    /// transferência não está de pé.
    ///
    /// `leitor` não é detalhe: o serviço tem mais autoridade que quem pede, e sem ele o serviço leria
    /// **por** quem pediu o que essa pessoa não leria sozinha (`ir_files::permissao`).
    ///
    /// Caminho vazio é descartado, e um pedido que fica sem nenhum é recusado aqui mesmo: é o único
    /// erro que se vê sem tocar o disco.
    /// Pedir de novo a **mesma** cópia que já está indo é aceito e não vira outra: é o Ctrl+C
    /// repetido de quem não viu retorno na tela. Pedir outra cancela a que está indo
    /// ([`fila::Fila`]).
    #[must_use]
    pub fn enviar(&self, caminhos: Vec<PathBuf>, leitor: Leitor) -> bool {
        let caminhos: Vec<PathBuf> = caminhos
            .into_iter()
            .filter(|caminho| !caminho.as_os_str().is_empty())
            .collect();
        if caminhos.is_empty() {
            return false;
        }
        let (recebido, saiu) = self.fila.pedir(caminhos, leitor);
        if let Some(trabalho) = saiu {
            retomada::espera_cancelada(&self.avisos, &trabalho);
        }
        if recebido == fila::Recebido::Repetido {
            debug!("esta cópia já está indo; não vai de novo");
            return true;
        }
        if self.acorda.send(()).is_err() {
            return false;
        }
        // Sem enlace, a cópia espera por ele — à vista, e com prazo. Sem par não há enlace a
        // esperar: quem conduz o canal recusa o pedido com esse motivo.
        let tem_par = self.destino.borrow().chave.is_some();
        if tem_par && !self.de_pe.load(Ordering::Acquire) {
            retomada::mostrar_espera(&self.fila, &self.avisos);
        }
        true
    }
}
