//! Quando o recado de uma cópia vai para a tela.
//!
//! O andamento chega cinco vezes por segundo. Um recado que se redesenha nesse ritmo é pior que
//! nenhum, e um que só aparece no começo e no fim fica parado em "0%" até acabar — a queixa que
//! levou o Linux a atualizar o recado no lugar. A regra, uma para os dois sistemas:
//!
//! - **O fim aparece sempre, na hora.** É o recado que importa: chegou, ou não atravessou.
//! - **A cópia rápida só tem o fim.** Durante o primeiro segundo nada aparece; uma cópia que acaba
//!   antes disso dá um recado só, e não um "copiando" seguido de um "pronto".
//! - **Depois, uma mudança de fase aparece na hora** — "esperando a conexão voltar" segurado pelo
//!   intervalo deixaria na tela um andamento que já parou —, e o andamento a cada intervalo.

use std::time::{Duration, Instant};

use ir_ipc::transferencia::Transferencia;

/// Quanto tempo uma cópia anda antes de ganhar recado.
const ESPERA: Duration = Duration::from_secs(1);

/// Uma vez de mostrar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vez {
    /// Se é o primeiro recado desta cópia: abre um novo, em vez de atualizar o que já está lá.
    pub primeira: bool,
}

/// Lembra o que já foi mostrado da cópia corrente.
#[derive(Debug)]
pub struct Ritmo {
    intervalo: Duration,
    /// Qual cópia: o nome e o sentido. O progresso não entra — é a mesma cópia andando.
    atual: Option<String>,
    /// Quando ela apareceu.
    inicio: Option<Instant>,
    /// Quando o último recado dela foi mostrado.
    mostrado: Option<Instant>,
    /// O título do último aviso, que muda quando a cópia muda de fase.
    fase: Option<&'static str>,
}

impl Ritmo {
    /// Um ritmo que atualiza o andamento a cada `intervalo`.
    #[must_use]
    pub const fn novo(intervalo: Duration) -> Self {
        Self {
            intervalo,
            atual: None,
            inicio: None,
            mostrado: None,
            fase: None,
        }
    }

    /// Se este aviso da cópia vai para a tela agora.
    pub fn vez(&mut self, copia: &Transferencia, agora: Instant) -> Option<Vez> {
        let chave = format!("{}|{}", copia.nome, copia.sentido.rotulo());
        if self.atual.as_deref() != Some(chave.as_str()) {
            self.atual = Some(chave);
            self.inicio = Some(agora);
            self.mostrado = None;
            self.fase = None;
        }
        let titulo = copia.titulo();
        let mudou_de_fase = self.fase.replace(titulo) != Some(titulo);
        let primeira = self.mostrado.is_none();
        if copia.terminou() {
            // O recado fechou esta cópia: a próxima, mesmo com o mesmo nome, começa outro.
            self.atual = None;
            return Some(Vez { primeira });
        }
        if self
            .inicio
            .is_some_and(|inicio| agora.duration_since(inicio) < ESPERA)
        {
            return None;
        }
        let passou = self
            .mostrado
            .is_none_or(|quando| agora.duration_since(quando) >= self.intervalo);
        if primeira || mudou_de_fase || passou {
            self.mostrado = Some(agora);
            return Some(Vez { primeira });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir_ipc::transferencia::{Fase, Motivo, Sentido};

    const INTERVALO: Duration = Duration::from_secs(2);

    fn copia(nome: &str, fase: Fase) -> Transferencia {
        Transferencia {
            sentido: Sentido::Recebendo,
            nome: nome.to_owned(),
            bytes_feitos: 10,
            bytes_total: 1000,
            fase,
        }
    }

    fn concluida(nome: &str) -> Transferencia {
        copia(
            nome,
            Fase::Concluida {
                destino: "/tmp/x".to_owned(),
            },
        )
    }

    #[test]
    fn a_copia_rapida_so_tem_o_fim() {
        let mut ritmo = Ritmo::novo(INTERVALO);
        let t0 = Instant::now();
        assert_eq!(ritmo.vez(&copia("a", Fase::Anunciada), t0), None);
        assert_eq!(ritmo.vez(&copia("a", Fase::Andando), t0 + ESPERA / 2), None);
        assert_eq!(
            ritmo.vez(&concluida("a"), t0 + ESPERA / 2),
            Some(Vez { primeira: true }),
            "um recado só, e é o do fim"
        );
    }

    /// O defeito do Linux: o recado nascia em "0%" e só mudava no fim.
    #[test]
    fn o_andamento_volta_a_aparecer_a_cada_intervalo() {
        let mut ritmo = Ritmo::novo(INTERVALO);
        let t0 = Instant::now();
        let andando = copia("a", Fase::Andando);
        ritmo.vez(&andando, t0);
        assert_eq!(
            ritmo.vez(&andando, t0 + ESPERA),
            Some(Vez { primeira: true })
        );
        assert_eq!(ritmo.vez(&andando, t0 + ESPERA + INTERVALO / 4), None);
        assert_eq!(
            ritmo.vez(&andando, t0 + ESPERA + INTERVALO),
            Some(Vez { primeira: false })
        );
        assert_eq!(
            ritmo.vez(&concluida("a"), t0 + ESPERA + INTERVALO),
            Some(Vez { primeira: false }),
            "o fim atualiza o mesmo recado"
        );
    }

    #[test]
    fn a_mudanca_de_fase_aparece_na_hora() {
        let mut ritmo = Ritmo::novo(INTERVALO);
        let t0 = Instant::now();
        ritmo.vez(&copia("a", Fase::Andando), t0);
        ritmo.vez(&copia("a", Fase::Andando), t0 + ESPERA);
        assert_eq!(
            ritmo.vez(
                &copia("a", Fase::AguardandoConexao),
                t0 + ESPERA + INTERVALO / 10
            ),
            Some(Vez { primeira: false }),
            "a espera pela conexão aparece na hora, e não no próximo intervalo"
        );
    }

    #[test]
    fn outra_copia_comeca_outro_recado_mesmo_com_o_mesmo_nome() {
        let mut ritmo = Ritmo::novo(INTERVALO);
        let t0 = Instant::now();
        ritmo.vez(&copia("a", Fase::Andando), t0);
        ritmo.vez(&copia("a", Fase::Andando), t0 + ESPERA);
        ritmo.vez(&copia("a", Fase::Parada(Motivo::Cancelada)), t0 + ESPERA);
        ritmo.vez(&copia("a", Fase::Andando), t0 + ESPERA * 2);
        assert_eq!(
            ritmo.vez(&copia("a", Fase::Andando), t0 + ESPERA * 3),
            Some(Vez { primeira: true })
        );
        assert_eq!(
            ritmo.vez(&copia("b", Fase::Andando), t0 + ESPERA * 3),
            None,
            "a outra cópia também espera o primeiro segundo"
        );
    }
}
