//! A velocidade da cópia em curso e a lista das últimas.
//!
//! "Está copiando?" é a pergunta do momento, e o cartão responde. "Aquilo copiou?" é a pergunta de
//! depois — quando a pessoa já está no outro computador — e quem responde é a lista: nome e o que
//! aconteceu com cada uma das últimas cópias, nada mais. E "quanto isto usou da minha rede?" é a
//! terceira, que o total da sessão responde.
//!
//! A velocidade é medida aqui, e não no serviço: o serviço conta bytes e diz quando; a taxa é uma
//! leitura disso no tempo, e é a tela que precisa dela. Medir entre dois avisos consecutivos daria
//! um número tremendo a cada 200 ms; a média móvel deixa o número legível sem mentir sobre a ordem
//! de grandeza.

use std::time::Instant;

use ir_ipc::transferencia::{Sentido, Transferencia};

use crate::gerado::ItemDeCopia;

/// Quantas cópias a lista guarda. As últimas dez cobrem "o que eu copiei agora há pouco?" sem virar
/// um arquivo de registro dentro da janela.
const LEMBRADAS: usize = 10;

/// O peso da medida nova na média móvel. Um terço: acompanha a mudança real em poucos avisos e
/// engole o solavanco de um bloco que demorou.
const PESO: f64 = 1.0 / 3.0;

/// A taxa de uma cópia, medida entre avisos.
#[derive(Debug, Default)]
pub struct Velocimetro {
    /// Quando e com quantos bytes foi a última medida.
    ultima: Option<(Instant, u64)>,
    /// A média móvel, em bytes por segundo.
    media: Option<f64>,
}

impl Velocimetro {
    /// Recomeça: é outra cópia.
    pub fn zerar(&mut self) {
        self.ultima = None;
        self.media = None;
    }

    /// Conta que a cópia está em `bytes` agora, e devolve a taxa para a tela.
    ///
    /// Vazio enquanto não houver duas medidas — um número inventado no primeiro aviso seria pior
    /// que nenhum.
    pub fn medir(&mut self, bytes: u64, agora: Instant) -> String {
        let anterior = self.ultima.replace((agora, bytes));
        let Some((quando, antes)) = anterior else {
            return String::new();
        };
        let segundos = agora.duration_since(quando).as_secs_f64();
        // Bytes que voltaram atrás são outra cópia usando o mesmo velocímetro; o tempo zerado é
        // dois avisos no mesmo instante. Nos dois casos, não há taxa a calcular agora.
        if segundos <= 0.0 || bytes < antes {
            return self.media.map(por_segundo).unwrap_or_default();
        }
        #[allow(clippy::cast_precision_loss)]
        let taxa = (bytes - antes) as f64 / segundos;
        let media = match self.media {
            Some(anterior) => anterior * (1.0 - PESO) + taxa * PESO,
            None => taxa,
        };
        self.media = Some(media);
        por_segundo(media)
    }
}

/// A taxa em unidade legível: "30,0 MB/s".
fn por_segundo(bytes_por_segundo: f64) -> String {
    const PASSO: f64 = 1024.0;
    const UNIDADES: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut valor = bytes_por_segundo.max(0.0);
    let mut unidade = 0;
    while valor >= PASSO && unidade + 1 < UNIDADES.len() {
        valor /= PASSO;
        unidade += 1;
    }
    let nome = UNIDADES.get(unidade).copied().unwrap_or("B/s");
    if unidade == 0 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return format!("{} {nome}", valor.round() as u64);
    }
    format!("{valor:.1} {nome}").replace('.', ",")
}

/// As últimas cópias, e quanto trafegou nesta sessão.
#[derive(Debug, Default)]
pub struct Historico {
    itens: Vec<ItemDeCopia>,
    /// Bytes que já atravessaram, nos dois sentidos, desde que a janela abriu.
    bytes: u64,
}

impl Historico {
    /// Guarda uma cópia que terminou. Ignora as que ainda estão andando.
    pub fn guardar(&mut self, copia: &Transferencia) {
        if !copia.terminou() {
            return;
        }
        self.bytes = self.bytes.saturating_add(copia.bytes_feitos);
        self.itens.insert(
            0,
            ItemDeCopia {
                nome: copia.nome.clone().into(),
                situacao: copia.detalhe().into(),
                estado: crate::copia::estado(copia),
                sentido: sentido(copia.sentido).into(),
            },
        );
        self.itens.truncate(LEMBRADAS);
    }

    /// As últimas, da mais recente para a mais antiga.
    #[must_use]
    pub fn itens(&self) -> &[ItemDeCopia] {
        &self.itens
    }

    /// Quanto o produto moveu nesta sessão, em texto — vazio enquanto não moveu nada.
    #[must_use]
    pub fn trafego(&self) -> String {
        if self.bytes == 0 {
            return String::new();
        }
        ir_ipc::transferencia::tamanho_legivel(self.bytes)
    }
}

/// Quanto os recebidos ocupam, na frase que a tela mostra, e se há o que limpar.
///
/// "nada guardado" em vez de "0 B": zero byte é um número; o que a pessoa quer saber é se há algo
/// ali ocupando espaço. Mora aqui, junto do tráfego da sessão, porque é o mesmo assunto — espaço
/// que o produto ocupa — e porque as duas frases têm de sair iguais.
#[must_use]
pub fn recebidos_ui(bytes: u64) -> (String, bool) {
    if bytes == 0 {
        return ("nada guardado".to_owned(), false);
    }
    (ir_ipc::transferencia::tamanho_legivel(bytes), true)
}

/// A palavra que diz para que lado a cópia foi.
const fn sentido(sentido: Sentido) -> &'static str {
    match sentido {
        Sentido::Enviando => "enviado",
        Sentido::Recebendo => "recebido",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use std::time::Duration;

    use ir_ipc::transferencia::{Fase, Motivo};

    use super::*;

    fn copia(nome: &str, fase: Fase, feitos: u64) -> Transferencia {
        Transferencia {
            sentido: Sentido::Enviando,
            nome: nome.to_owned(),
            bytes_feitos: feitos,
            bytes_total: 100 * 1024 * 1024,
            fase,
        }
    }

    #[test]
    fn a_primeira_medida_nao_inventa_velocidade() {
        let mut velocimetro = Velocimetro::default();
        assert_eq!(velocimetro.medir(1024, Instant::now()), "");
    }

    #[test]
    fn dez_megabytes_em_um_segundo_sao_dez_megabytes_por_segundo() {
        let inicio = Instant::now();
        let mut velocimetro = Velocimetro::default();
        velocimetro.medir(0, inicio);
        let taxa = velocimetro.medir(10 * 1024 * 1024, inicio + Duration::from_secs(1));
        assert_eq!(taxa, "10,0 MB/s");
    }

    #[test]
    fn a_media_movel_acompanha_a_mudanca_sem_saltar() {
        let inicio = Instant::now();
        let mut velocimetro = Velocimetro::default();
        velocimetro.medir(0, inicio);
        // Um segundo a 30 MB/s, e depois um a 0: a média cai, mas não para zero de uma vez.
        velocimetro.medir(30 * 1024 * 1024, inicio + Duration::from_secs(1));
        let taxa = velocimetro.medir(30 * 1024 * 1024, inicio + Duration::from_secs(2));
        assert_eq!(taxa, "20,0 MB/s");
    }

    #[test]
    fn outra_copia_no_mesmo_velocimetro_nao_vira_taxa_negativa() {
        let inicio = Instant::now();
        let mut velocimetro = Velocimetro::default();
        velocimetro.medir(50 * 1024 * 1024, inicio);
        let taxa = velocimetro.medir(1024, inicio + Duration::from_secs(1));
        assert_eq!(taxa, "", "sem medida anterior válida, nada a mostrar");
    }

    #[test]
    fn a_lista_guarda_so_o_que_terminou_e_a_mais_nova_primeiro() {
        let mut historico = Historico::default();
        historico.guardar(&copia("andando", Fase::Andando, 10));
        assert!(
            historico.itens().is_empty(),
            "o que anda não entra na lista"
        );

        historico.guardar(&copia(
            "primeira",
            Fase::Concluida {
                destino: String::new(),
            },
            10,
        ));
        historico.guardar(&copia("segunda", Fase::Parada(Motivo::Cancelada), 5));
        let nomes: Vec<&str> = historico.itens().iter().map(|i| i.nome.as_str()).collect();
        assert_eq!(nomes, ["segunda", "primeira"]);
    }

    #[test]
    fn a_lista_nao_cresce_sem_fim() {
        let mut historico = Historico::default();
        for i in 0..25 {
            historico.guardar(&copia(
                &format!("copia-{i}"),
                Fase::Concluida {
                    destino: String::new(),
                },
                1,
            ));
        }
        assert_eq!(historico.itens().len(), LEMBRADAS);
        assert_eq!(historico.itens()[0].nome.as_str(), "copia-24");
    }

    #[test]
    fn o_trafego_soma_os_dois_sentidos_da_sessao() {
        let mut historico = Historico::default();
        assert_eq!(historico.trafego(), "", "nada moveu ainda");
        let pronta = Fase::Concluida {
            destino: String::new(),
        };
        historico.guardar(&copia("a", pronta.clone(), 512 * 1024));
        let mut recebida = copia("b", pronta, 512 * 1024);
        recebida.sentido = Sentido::Recebendo;
        historico.guardar(&recebida);
        assert_eq!(historico.trafego(), "1,0 MB");
    }
}
