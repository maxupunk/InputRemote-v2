//! A linha que amostra a cada segundo, enquanto alguém olha.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use ir_monitor::processos::{InteresseEmProcessos, Processos};
use ir_proto::message::UsageSample;

use crate::coletor::Coletor;
use crate::fonte::Agendada;
use crate::processos::Medidor;

/// De quanto em quanto tempo sai uma amostra. Decisão de 2026-10-24
/// ([12, §2](../../../docs/12-painel-android.md)): a latência e o processador a 2 s pareciam
/// atrasados, e a amostra cabe folgada em 1 s.
pub const PERIODO: Duration = Duration::from_secs(1);

/// O que a linha mede: as fontes da amostra e, onde há, o medidor de processos.
pub type Equipamento = (Vec<Agendada>, Option<Medidor>);

/// Enquanto viver, entrega uma amostra por [`PERIODO`] — e os processos, quando há interesse.
///
/// Soltar a vigia para a linha na próxima espera — sem `join`, para quem solta (o laço do serviço)
/// não esperar uma consulta ao WMI terminar. As fontes nascem **dentro** da linha: a conexão COM do
/// WMI pertence à linha que a abriu.
#[derive(Debug)]
pub struct Vigia {
    interesses: mpsc::Sender<InteresseEmProcessos>,
}

impl Vigia {
    /// Começa a amostrar esta máquina. `entregar` recebe cada amostra, com os processos quando há
    /// interesse neles, e devolve se ainda quer a próxima; `false` encerra a linha.
    ///
    /// # Errors
    ///
    /// Os do sistema ao criar a linha.
    pub fn iniciar(
        entregar: impl FnMut(UsageSample, Option<Processos>) -> bool + Send + 'static,
    ) -> std::io::Result<Self> {
        let equipamento = || {
            (
                crate::fontes_desta_maquina(),
                crate::medidor_desta_maquina(),
            )
        };
        Self::iniciar_com(equipamento, PERIODO, entregar)
    }

    /// Como [`Self::iniciar`], com o equipamento e o período dados — para teste.
    ///
    /// # Errors
    ///
    /// Os do sistema ao criar a linha.
    pub fn iniciar_com(
        equipamento: impl FnOnce() -> Equipamento + Send + 'static,
        periodo: Duration,
        entregar: impl FnMut(UsageSample, Option<Processos>) -> bool + Send + 'static,
    ) -> std::io::Result<Self> {
        let (interesses, ouvir) = mpsc::channel();
        thread::Builder::new()
            .name("ir-telemetria".to_owned())
            .spawn(move || {
                let (fontes, medidor) = equipamento();
                laco(&ouvir, Coletor::novo(fontes), medidor, periodo, entregar);
            })?;
        Ok(Self { interesses })
    }

    /// Quais processos medir daqui em diante. Vazio, nenhum: os processos não são lidos.
    pub fn interessar(&self, interesse: InteresseEmProcessos) {
        // A linha só acaba quando a vigia é solta ou quem recebe desiste; nos dois casos, não há a
        // quem avisar.
        let _ = self.interesses.send(interesse);
    }
}

fn laco(
    ouvir: &mpsc::Receiver<InteresseEmProcessos>,
    mut coletor: Coletor,
    mut medidor: Option<Medidor>,
    periodo: Duration,
    mut entregar: impl FnMut(UsageSample, Option<Processos>) -> bool,
) {
    tracing::debug!("vigia de uso começou");
    let mut interesse = InteresseEmProcessos::default();
    loop {
        let processos = medidor
            .as_mut()
            .filter(|_| !interesse.vazio())
            .and_then(|m| m.medir(&interesse));
        if !entregar(coletor.amostrar(Instant::now()), processos) {
            break;
        }
        // A vigia foi solta: acabou.
        let Ok(novo) = esperar(ouvir, periodo) else {
            break;
        };
        if let Some(novo) = novo {
            if novo.vazio() {
                medidor.as_mut().map(Medidor::esquecer);
            }
            interesse = novo;
        }
    }
    tracing::debug!("vigia de uso parou");
}

/// Espera o período inteiro, guardando o último interesse que chegar. O erro é a vigia solta.
fn esperar(
    ouvir: &mpsc::Receiver<InteresseEmProcessos>,
    periodo: Duration,
) -> Result<Option<InteresseEmProcessos>, RecvTimeoutError> {
    let fim = Instant::now() + periodo;
    let mut ultimo = None;
    loop {
        match ouvir.recv_timeout(fim.saturating_duration_since(Instant::now())) {
            Ok(interesse) => ultimo = Some(interesse),
            Err(RecvTimeoutError::Timeout) => return Ok(ultimo),
            Err(solta) => return Err(solta),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonte::Fonte;
    use crate::processos::{Foto, Fotografo};

    struct Fixa;

    impl Fonte for Fixa {
        fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
            amostra.cpu_permille = Some(42);
        }
    }

    struct Vazia;

    impl Fotografo for Vazia {
        fn fotografar(&mut self) -> Option<Foto> {
            Some(Foto::default())
        }
    }

    #[test]
    fn entrega_amostras_e_para_quando_solta() {
        let (tx, rx) = mpsc::channel();
        let vigia = Vigia::iniciar_com(
            || (vec![Agendada::nova(Fixa, 1)], None),
            Duration::from_millis(5),
            move |amostra, _| tx.send(amostra).is_ok(),
        )
        .unwrap();
        let primeira = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(primeira.cpu_permille, Some(42));
        drop(vigia);
        // Depois de solta, a linha acaba e o canal fecha.
        let fim = Instant::now() + Duration::from_secs(2);
        while rx.recv_timeout(Duration::from_millis(50)).is_ok() {
            assert!(Instant::now() < fim, "a linha não parou");
        }
    }

    #[test]
    fn quem_recebe_pode_encerrar() {
        let (tx, rx) = mpsc::channel();
        let _vigia = Vigia::iniciar_com(
            || (Vec::new(), None),
            Duration::from_millis(1),
            move |_, _| {
                let _ = tx.send(());
                false
            },
        )
        .unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok());
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn os_processos_so_vem_com_interesse() {
        let (tx, rx) = mpsc::channel();
        let vigia = Vigia::iniciar_com(
            || (Vec::new(), Some(Medidor::novo(Vazia))),
            Duration::from_millis(5),
            move |_, processos| tx.send(processos.is_some()).is_ok(),
        )
        .unwrap();
        let receber = || rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!receber(), "sem interesse, nada de processos");
        vigia.interessar(InteresseEmProcessos {
            nomes: Vec::new(),
            lista: true,
        });
        let fim = Instant::now() + Duration::from_secs(2);
        while !receber() {
            assert!(Instant::now() < fim, "os processos não vieram");
        }
        vigia.interessar(InteresseEmProcessos::default());
        let fim = Instant::now() + Duration::from_secs(2);
        while receber() {
            assert!(Instant::now() < fim, "os processos não pararam");
        }
    }
}
