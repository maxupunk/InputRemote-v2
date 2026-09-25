//! O relatório de diagnóstico, para copiar e anexar a um relato.
//!
//! Montado campo a campo, por uma lista do que **pode** sair — e não filtrando o que não pode
//! ([04, §7](../../../docs/04-seguranca.md)): nada do que foi digitado, nenhum caminho de arquivo.
//! Traz o que Preferências promete — meio de conexão, atraso medido, motivo da última queda e
//! nível —, que antes não vinha.

use ir_ipc::Estado;

/// O que o relatório traz além do estado publicado: o que só o serviço sabe dizer.
#[derive(Debug, Clone)]
pub struct Extras {
    /// A versão de protocolo que o par fala, ou "sem sessão".
    pub par_versao: String,
    /// A rota, com o placar de quem chega primeiro.
    pub rota: String,
    /// A fase da sessão, pelo nome dela.
    pub fase: String,
    /// Onde o par está em cada portador.
    pub alcance: String,
    /// O rádio desta máquina.
    pub radio: String,
    /// Os desktops em que o agente injeta.
    pub desktops: String,
    /// A economia do Wi-Fi, daqui e do par, como o serviço leu.
    pub economia: String,
    /// Quantos pares estão gravados.
    pub pares_gravados: usize,
    /// Quantos ajudantes de clipboard estão ligados.
    pub ajudantes: usize,
}

/// O relatório, uma linha por campo.
#[must_use]
pub fn relatorio(estado: &Estado, extras: &Extras) -> String {
    let latencia = estado.latencia.map_or_else(
        || "sem medida".to_owned(),
        |medida| medida.frase_do_diagnostico(),
    );
    let queda = estado
        .ultima_queda
        .map_or("nenhuma nesta execução", |motivo| motivo.frase());
    let nivel = estado.nivel_privilegiado;
    let linhas = [
        format!("versão do produto: {}", env!("CARGO_PKG_VERSION")),
        format!(
            "protocolo: {} (o par fala {})",
            ir_proto::version::CURRENT,
            extras.par_versao
        ),
        format!("política: {:?}", estado.politica),
        format!("fase: {}", extras.fase),
        format!("pausa: {:?}", estado.pausa),
        format!("rota: {}", extras.rota),
        format!("atraso: {latencia}"),
        format!("última queda: {queda}"),
        format!("nível: {} ({})", nivel.rotulo(), nivel.explicacao()),
        format!(
            "tela de bloqueio permitida ao par: {}",
            estado.bloqueio_permitido
        ),
        format!("recebe teclado e mouse: {}", estado.agente_pronto),
        format!("lê o próprio teclado e mouse: {}", estado.captura_pronta),
        format!("desktops do agente: {}", extras.desktops),
        format!("pares gravados: {}", extras.pares_gravados),
        format!("par: {}", extras.alcance),
        format!("rádio Bluetooth: {}", extras.radio),
        format!("economia do Wi-Fi: {}", extras.economia),
        format!("ajudantes de clipboard ligados: {}", extras.ajudantes),
    ];
    linhas.join("\n")
}

#[cfg(test)]
mod tests {
    use ir_ipc::{Maquina, Nome};

    use super::*;

    #[test]
    fn o_relatorio_traz_o_que_preferencias_promete_sem_buracos() {
        let estado = Estado::recem_instalado(Maquina([2; 16]), Nome::coagido("bancada"));
        let extras = Extras {
            par_versao: "sem sessão".to_owned(),
            rota: "nenhuma".to_owned(),
            fase: "desconectado".to_owned(),
            alcance: "rede desconhecido".to_owned(),
            radio: "indisponível".to_owned(),
            desktops: "[]".to_owned(),
            economia: "Desconhecida".to_owned(),
            pares_gravados: 0,
            ajudantes: 0,
        };
        let relatorio = relatorio(&estado, &extras);
        for campo in ["atraso:", "última queda:", "nível:", "rota:", "protocolo:"] {
            assert!(relatorio.contains(campo), "falta `{campo}`:\n{relatorio}");
        }
        for linha in relatorio.lines() {
            assert!(!linha.starts_with(' '), "linha com buraco: `{linha}`");
        }
    }
}
