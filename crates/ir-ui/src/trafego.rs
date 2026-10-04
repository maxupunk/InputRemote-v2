//! O tráfego da tela inicial: "está fazendo alguma coisa agora?".
//!
//! O serviço conta os bytes que o canal de dados move — cópias e pastas compartilhadas, nos dois
//! sentidos — e a janela lê essa contagem a cada segundo, junto com o estado. A taxa é a diferença
//! entre duas leituras.
//!
//! Uma linha só, e só o que responde a pergunta: "↓ 2,4 MB/s ↑ 12 kB/s" enquanto algo atravessa,
//! "parado" quando nada atravessa. O sentido que está parado não aparece. Não há total nem gráfico:
//! o total não diz se algo está acontecendo agora, e o gráfico já é do atraso.
//!
//! Distinto do velocímetro da cópia ([`crate::historico::Velocimetro`]): aquele suaviza bastante,
//! para a velocidade de uma cópia ser legível; aqui, quando nada passou no último intervalo, a linha
//! diz "parado" na hora — um "1 kB/s" que demora vinte segundos para sumir faria parecer que algo
//! continua acontecendo.

use std::time::Instant;

use ir_ipc::Trafego;

/// Abaixo disto é ruído — a conversa de manutenção do canal —, e não um trabalho em curso.
const PARADO: f64 = 1024.0;

/// O peso da leitura nova: segue a mudança real em duas leituras e engole um solavanco.
const PESO: f64 = 0.5;

/// Leituras mais próximas que isto não medem: a janela também lê o estado fora do compasso.
const INTERVALO_MINIMO: f64 = 0.5;

/// A taxa do canal de dados, nos dois sentidos.
#[derive(Debug, Default)]
pub struct MedidorDeTrafego {
    anterior: Option<(Instant, Trafego)>,
    descendo: f64,
    subindo: f64,
}

impl MedidorDeTrafego {
    /// Conta a leitura de agora e devolve a linha da tela, e se algo está atravessando.
    pub fn medir(&mut self, trafego: Trafego, agora: Instant) -> (String, bool) {
        match self.anterior {
            None => self.anterior = Some((agora, trafego)),
            Some((quando, antes)) => {
                let segundos = agora.duration_since(quando).as_secs_f64();
                if segundos >= INTERVALO_MINIMO {
                    self.descendo = suavizar(
                        self.descendo,
                        taxa(antes.recebidos, trafego.recebidos, segundos),
                    );
                    self.subindo = suavizar(
                        self.subindo,
                        taxa(antes.enviados, trafego.enviados, segundos),
                    );
                    self.anterior = Some((agora, trafego));
                }
            }
        }
        frase(self.descendo, self.subindo)
    }
}

/// Bytes por segundo entre duas contagens. Uma contagem que voltou atrás é o serviço que
/// reiniciou: não é taxa nenhuma.
fn taxa(antes: u64, agora: u64, segundos: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)] // a fração de byte não aparece na tela
    let bytes = agora.saturating_sub(antes) as f64;
    bytes / segundos
}

/// Nada passou: zero na hora. Senão, a média das duas.
fn suavizar(anterior: f64, nova: f64) -> f64 {
    if nova <= 0.0 {
        0.0
    } else {
        anterior * (1.0 - PESO) + nova * PESO
    }
}

/// A linha da tela: os sentidos que estão andando, ou "parado".
fn frase(descendo: f64, subindo: f64) -> (String, bool) {
    let partes: Vec<String> = [("↓", descendo), ("↑", subindo)]
        .into_iter()
        .filter(|(_, taxa)| *taxa >= PARADO)
        .map(|(seta, taxa)| format!("{seta} {}", crate::historico::por_segundo(taxa)))
        .collect();
    if partes.is_empty() {
        ("parado".to_owned(), false)
    } else {
        (partes.join("  "), true)
    }
}

#[cfg(test)]
mod testes {
    use std::time::Duration;

    use super::*;

    fn lido(enviados: u64, recebidos: u64) -> Trafego {
        Trafego {
            enviados,
            recebidos,
        }
    }

    #[test]
    fn mostra_so_o_sentido_que_anda_e_parado_quando_nada_anda() {
        let mut medidor = MedidorDeTrafego::default();
        let inicio = Instant::now();
        assert_eq!(
            medidor.medir(lido(0, 0), inicio),
            ("parado".to_owned(), false)
        );

        let um = inicio + Duration::from_secs(1);
        let (linha, ativo) = medidor.medir(lido(500, 2_000_000), um);
        assert!(ativo);
        assert!(linha.starts_with('↓'), "{linha}");
        assert!(
            !linha.contains('↑'),
            "500 B/s é conversa do canal, não trabalho: {linha}"
        );

        // Nada passou no último segundo: "parado" na hora, e não aos poucos.
        let dois = um + Duration::from_secs(1);
        assert_eq!(
            medidor.medir(lido(500, 2_000_000), dois),
            ("parado".to_owned(), false)
        );
    }

    #[test]
    fn os_dois_sentidos_juntos_e_leitura_fora_do_compasso_nao_mede() {
        let mut medidor = MedidorDeTrafego::default();
        let inicio = Instant::now();
        medidor.medir(lido(0, 0), inicio);
        let (linha, _) = medidor.medir(lido(3_000_000, 3_000_000), inicio + Duration::from_secs(1));
        assert!(linha.contains('↓') && linha.contains('↑'), "{linha}");
        // Uma leitura 100 ms depois não vira taxa nova: o número não pula entre duas medidas.
        let depois = medidor.medir(
            lido(3_000_000, 3_000_000),
            inicio + Duration::from_millis(1100),
        );
        assert_eq!(depois.0, linha);
    }

    #[test]
    fn o_servico_que_reiniciou_nao_vira_taxa_negativa() {
        let mut medidor = MedidorDeTrafego::default();
        let inicio = Instant::now();
        medidor.medir(lido(9_000_000, 9_000_000), inicio);
        let (linha, ativo) = medidor.medir(lido(0, 0), inicio + Duration::from_secs(1));
        assert_eq!((linha.as_str(), ativo), ("parado", false));
    }
}
