//! A linha que amostra a cada segundo, enquanto alguém olha.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use ir_proto::message::UsageSample;

use crate::coletor::Coletor;
use crate::fonte::Agendada;

/// De quanto em quanto tempo sai uma amostra. Decisão de 2026-10-24
/// ([12, §2](../../../docs/12-painel-android.md)): a latência e o processador a 2 s pareciam
/// atrasados, e a amostra cabe folgada em 1 s.
pub const PERIODO: Duration = Duration::from_secs(1);

/// Enquanto viver, entrega uma amostra por [`PERIODO`].
///
/// Soltar a vigia para a linha na próxima espera — sem `join`, para quem solta (o laço do serviço)
/// não esperar uma consulta ao WMI terminar. As fontes nascem **dentro** da linha: a conexão COM do
/// WMI pertence à linha que a abriu.
#[derive(Debug)]
pub struct Vigia {
    _parar: mpsc::Sender<()>,
}

impl Vigia {
    /// Começa a amostrar esta máquina. `entregar` recebe cada amostra e devolve se ainda quer a
    /// próxima; `false` encerra a linha.
    ///
    /// # Errors
    ///
    /// Os do sistema ao criar a linha.
    pub fn iniciar(
        entregar: impl FnMut(UsageSample) -> bool + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::iniciar_com(crate::fontes_desta_maquina, PERIODO, entregar)
    }

    /// Como [`Self::iniciar`], com as fontes e o período dados — para teste.
    ///
    /// # Errors
    ///
    /// Os do sistema ao criar a linha.
    pub fn iniciar_com(
        fontes: impl FnOnce() -> Vec<Agendada> + Send + 'static,
        periodo: Duration,
        entregar: impl FnMut(UsageSample) -> bool + Send + 'static,
    ) -> std::io::Result<Self> {
        let (parar, ouvir) = mpsc::channel();
        thread::Builder::new()
            .name("ir-telemetria".to_owned())
            .spawn(move || laco(&ouvir, Coletor::novo(fontes()), periodo, entregar))?;
        Ok(Self { _parar: parar })
    }
}

fn laco(
    ouvir: &mpsc::Receiver<()>,
    mut coletor: Coletor,
    periodo: Duration,
    mut entregar: impl FnMut(UsageSample) -> bool,
) {
    tracing::debug!("vigia de uso começou");
    loop {
        if !entregar(coletor.amostrar(Instant::now())) {
            break;
        }
        match ouvir.recv_timeout(periodo) {
            Err(RecvTimeoutError::Timeout) => {}
            // A vigia foi solta (ou alguém mandou parar): acabou.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    tracing::debug!("vigia de uso parou");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonte::Fonte;

    struct Fixa;

    impl Fonte for Fixa {
        fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
            amostra.cpu_permille = Some(42);
        }
    }

    #[test]
    fn entrega_amostras_e_para_quando_solta() {
        let (tx, rx) = mpsc::channel();
        let vigia = Vigia::iniciar_com(
            || vec![Agendada::nova(Fixa, 1)],
            Duration::from_millis(5),
            move |amostra| tx.send(amostra).is_ok(),
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
        let _vigia = Vigia::iniciar_com(Vec::new, Duration::from_millis(1), move |_| {
            let _ = tx.send(());
            false
        })
        .unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok());
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }
}
