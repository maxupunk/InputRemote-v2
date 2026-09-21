//! Quem garante que o ajudante de clipboard exista, no Windows.
//!
//! O ajudante nascia pela chave `Run`, só no login: instalar ou atualizar com o usuário dentro
//! deixava copiar e colar parado, sem aviso, até o próximo — duas vezes. Agora o serviço o lança na
//! sessão de console, com o token de quem entrou, sempre que ele faltar. No Linux quem faz isso é o
//! `systemd` do usuário (`Restart=always`), que sabe quando há sessão gráfica.

use std::time::{Duration, Instant};

#[cfg(windows)]
use tracing::{debug, info};

use crate::Ajudantes;

/// A cada quanto se confere.
#[cfg_attr(not(windows), allow(dead_code))]
const CONFERIR: Duration = Duration::from_secs(5);

/// Quanto tempo sem ajudante se tolera antes de lançar um: bem mais que a reconexão dele (2 s), para
/// o ajudante vivo voltar sozinho quando é o serviço que acabou de subir.
#[cfg_attr(not(windows), allow(dead_code))]
const TOLERANCIA: Duration = Duration::from_secs(12);

/// A decisão, separada do relógio e do sistema para ser testada.
// A decisão é a mesma nos dois sistemas, e o teste dela vale nos dois — mas quem a usa é só o
// lançamento do Windows. No Linux ela compila, fica testada, e ninguém a chama.
#[derive(Debug, Default)]
#[cfg_attr(not(windows), allow(dead_code))]
struct Zelador {
    /// Desde quando não há ajudante ligado.
    ausente_desde: Option<Instant>,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Zelador {
    /// Se é hora de lançar um ajudante, dado quantos estão ligados agora.
    ///
    /// Depois de mandar lançar, a contagem recomeça: o lançado tem a mesma tolerância para ligar.
    fn conferir(&mut self, ligados: usize, agora: Instant) -> bool {
        if ligados > 0 {
            self.ausente_desde = None;
            return false;
        }
        let desde = *self.ausente_desde.get_or_insert(agora);
        if agora.duration_since(desde) < TOLERANCIA {
            return false;
        }
        self.ausente_desde = Some(agora);
        true
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
        let mut zelador = Zelador::default();
        let mut ultimo_erro = String::new();
        loop {
            tokio::time::sleep(CONFERIR).await;
            if !zelador.conferir(ajudantes.ligados(), Instant::now()) {
                continue;
            }
            match crate::lancador::lancar_ajudante_de_clipboard() {
                Ok(pid) => {
                    info!(pid, "ajudante de clipboard lançado na sessão do usuário");
                    ultimo_erro.clear();
                }
                // Ninguém entrou ainda é o caso comum (a tela de login); registrar a cada 12 s
                // encheria o arquivo. Cada motivo novo aparece uma vez.
                Err(erro) => {
                    let texto = format!("{erro:#}");
                    if texto == ultimo_erro {
                        debug!(erro = %texto, "ajudante de clipboard ainda sem sessão");
                    } else {
                        info!(erro = %texto, "ainda não há como lançar o ajudante de clipboard");
                        ultimo_erro = texto;
                    }
                }
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
    use super::*;

    #[test]
    fn com_ajudante_ligado_nao_lanca_nada() {
        let mut zelador = Zelador::default();
        let inicio = Instant::now();
        for segundos in [0, 30, 600] {
            assert!(!zelador.conferir(1, inicio + Duration::from_secs(segundos)));
        }
    }

    #[test]
    fn sem_ajudante_lanca_depois_da_tolerancia_e_espera_o_lancado() {
        let mut zelador = Zelador::default();
        let t0 = Instant::now();
        assert!(
            !zelador.conferir(0, t0),
            "o ajudante vivo pode estar reconectando"
        );
        assert!(!zelador.conferir(0, t0 + Duration::from_secs(5)));
        assert!(zelador.conferir(0, t0 + TOLERANCIA));
        // O lançado ganha a mesma folga para ligar, em vez de ser lançado de novo a cada conferida.
        assert!(!zelador.conferir(0, t0 + TOLERANCIA + Duration::from_secs(5)));
        assert!(zelador.conferir(0, t0 + TOLERANCIA * 2));
    }

    #[test]
    fn um_ajudante_que_volta_zera_a_contagem() {
        let mut zelador = Zelador::default();
        let t0 = Instant::now();
        assert!(!zelador.conferir(0, t0));
        assert!(!zelador.conferir(1, t0 + Duration::from_secs(10)));
        assert!(!zelador.conferir(0, t0 + Duration::from_secs(11)));
        assert!(!zelador.conferir(0, t0 + Duration::from_secs(20)));
        assert!(zelador.conferir(0, t0 + Duration::from_secs(23)));
    }
}
