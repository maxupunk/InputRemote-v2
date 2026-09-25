//! Quem garante que um processo do produto exista na sessão do usuário, no Windows.
//!
//! Dois processos precisam disso, e o [`Zelador`] é o mesmo para os dois: o **ajudante de
//! clipboard**, zelado aqui ([`zelar_pelo_clipboard`]), e o **agente**, zelado pelo ator do serviço,
//! que é quem sabe se ele está pronto. Antes cada um tinha a própria regra: o ajudante tolerava a
//! ausência e registrava cada motivo de falha uma vez; o agente registrava um aviso a cada tentativa,
//! o que na tela de login enchia o registro com a mesma frase.
//!
//! O ajudante nascia pela chave `Run`, só no login: instalar ou atualizar com o usuário dentro
//! deixava copiar e colar parado, sem aviso, até o próximo — duas vezes. Agora o serviço o lança na
//! sessão de console, com o token de quem entrou, sempre que ele faltar. No Linux quem faz isso é o
//! `systemd` do usuário (`Restart=always`), que sabe quando há sessão gráfica.

use std::time::{Duration, Instant};

use tracing::{debug, info};

use crate::Ajudantes;

/// A cada quanto se confere o ajudante.
#[cfg_attr(not(windows), allow(dead_code))]
const CONFERIR: Duration = Duration::from_secs(5);

/// Quanto tempo sem ajudante se tolera antes de lançar um: bem mais que a reconexão dele (2 s), para
/// o ajudante vivo voltar sozinho quando é o serviço que acabou de subir.
#[cfg_attr(not(windows), allow(dead_code))]
const TOLERANCIA: Duration = Duration::from_secs(12);

/// Quando lançar de novo um processo que deveria estar de pé, e o registro de cada tentativa.
///
/// A decisão é separada do relógio e do sistema para ser testada; o lançamento entra por parâmetro.
#[derive(Debug)]
pub struct Zelador {
    /// Quem é zelado, para o registro.
    quem: &'static str,
    /// Quanto tempo de ausência se tolera, e quanto se espera o lançado aparecer.
    tolerancia: Duration,
    /// Desde quando o processo está ausente.
    ausente_desde: Option<Instant>,
    /// O motivo da última falha, para cada motivo novo aparecer uma vez só no registro.
    ultimo_erro: String,
}

impl Zelador {
    /// Um zelador de `quem`, que tolera `tolerancia` de ausência antes de lançar de novo.
    #[must_use]
    pub const fn novo(quem: &'static str, tolerancia: Duration) -> Self {
        Self {
            quem,
            tolerancia,
            ausente_desde: None,
            ultimo_erro: String::new(),
        }
    }

    /// Se é hora de lançar, dado se o processo está presente agora.
    ///
    /// Depois de mandar lançar, a contagem recomeça: o lançado tem a mesma tolerância para aparecer.
    pub fn conferir(&mut self, presente: bool, agora: Instant) -> bool {
        if presente {
            self.ausente_desde = None;
            return false;
        }
        let desde = *self.ausente_desde.get_or_insert(agora);
        if agora.saturating_duration_since(desde) < self.tolerancia {
            return false;
        }
        self.ausente_desde = Some(agora);
        true
    }

    /// Lança agora, com `lancar`, e recomeça a contagem: o lançado ganha a tolerância inteira.
    ///
    /// Ninguém ter entrado ainda é o caso comum (a tela de login), e registrá-lo a cada tentativa
    /// encheria o arquivo: cada motivo novo aparece uma vez, e a repetição fica no `debug`.
    pub fn lancar(&mut self, lancar: impl FnOnce() -> anyhow::Result<u32>, agora: Instant) {
        self.ausente_desde = Some(agora);
        match lancar() {
            Ok(pid) => {
                info!(pid, "{} lançado na sessão do usuário", self.quem);
                self.ultimo_erro.clear();
            }
            Err(erro) => {
                let texto = format!("{erro:#}");
                if texto == self.ultimo_erro {
                    debug!(erro = %texto, "{} ainda sem como ser lançado", self.quem);
                } else {
                    info!(erro = %texto, "ainda não há como lançar o {}", self.quem);
                    self.ultimo_erro = texto;
                }
            }
        }
    }
}

/// Sobe a tarefa que zela pelo ajudante. Só como serviço; em primeiro plano (teste à mão), não faz
/// nada — quem testa sobe o ajudante que quiser.
#[cfg(windows)]
pub fn zelar_pelo_clipboard(ajudantes: Ajudantes) {
    if !crate::lancador::como_servico() {
        return;
    }
    tokio::spawn(async move {
        let mut zelador = Zelador::novo("ajudante de clipboard", TOLERANCIA);
        loop {
            tokio::time::sleep(CONFERIR).await;
            let agora = Instant::now();
            if zelador.conferir(ajudantes.ligados() > 0, agora) {
                zelador.lancar(crate::lancador::lancar_ajudante_de_clipboard, agora);
            }
        }
    });
}

/// Fora do Windows quem zela pelo ajudante é o `systemd` do usuário, com `Restart=always` na
/// unidade que o pacote instala: ele sabe quando existe sessão gráfica, e este crate não.
#[cfg(not(windows))]
pub fn zelar_pelo_clipboard(_ajudantes: Ajudantes) {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn zelador() -> Zelador {
        Zelador::novo("teste", TOLERANCIA)
    }

    #[test]
    fn com_ajudante_ligado_nao_lanca_nada() {
        let mut zelador = zelador();
        let inicio = Instant::now();
        for segundos in [0, 30, 600] {
            assert!(!zelador.conferir(true, inicio + Duration::from_secs(segundos)));
        }
    }

    #[test]
    fn sem_ajudante_lanca_depois_da_tolerancia_e_espera_o_lancado() {
        let mut zelador = zelador();
        let t0 = Instant::now();
        assert!(
            !zelador.conferir(false, t0),
            "o ajudante vivo pode estar reconectando"
        );
        assert!(!zelador.conferir(false, t0 + Duration::from_secs(5)));
        assert!(zelador.conferir(false, t0 + TOLERANCIA));
        // O lançado ganha a mesma folga para ligar, em vez de ser lançado de novo a cada conferida.
        assert!(!zelador.conferir(false, t0 + TOLERANCIA + Duration::from_secs(5)));
        assert!(zelador.conferir(false, t0 + TOLERANCIA * 2));
    }

    #[test]
    fn um_ajudante_que_volta_zera_a_contagem() {
        let mut zelador = zelador();
        let t0 = Instant::now();
        assert!(!zelador.conferir(false, t0));
        assert!(!zelador.conferir(true, t0 + Duration::from_secs(10)));
        assert!(!zelador.conferir(false, t0 + Duration::from_secs(11)));
        assert!(!zelador.conferir(false, t0 + Duration::from_secs(20)));
        assert!(zelador.conferir(false, t0 + Duration::from_secs(23)));
    }

    #[test]
    fn lancar_a_mao_recomeca_a_contagem_e_guarda_o_motivo() {
        let mut zelador = zelador();
        let t0 = Instant::now();
        zelador.lancar(|| Err(anyhow::anyhow!("ninguém entrou")), t0);
        assert_eq!(zelador.ultimo_erro, "ninguém entrou");
        assert!(
            !zelador.conferir(false, t0 + Duration::from_secs(11)),
            "o lançado ainda tem folga"
        );
        assert!(zelador.conferir(false, t0 + TOLERANCIA));
        zelador.lancar(|| Ok(42), t0 + TOLERANCIA);
        assert!(
            zelador.ultimo_erro.is_empty(),
            "deu certo: o próximo erro volta a aparecer"
        );
    }
}
