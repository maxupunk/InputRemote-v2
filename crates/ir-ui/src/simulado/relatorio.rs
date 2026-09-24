//! O relatório de diagnóstico do serviço simulado.

use ir_ipc::status::{Estado, MotivoDaQueda, MotivoDoPortador, Politica};
use ir_ipc::vocabulario::Portador;

/// O relatório de diagnóstico.
///
/// Lista fechada, montada campo por campo. **Nunca** despeja estado inteiro: o produto vê senhas, e
/// um relatório que o usuário cola num relato público não pode conter nada do que foi digitado
/// ([04, §7](../../../docs/04-seguranca.md)).
pub(super) fn diagnostico(estado: &Estado) -> String {
    let campos: [(&str, String); 12] = [
        ("maquina", estado.este_nome.como_texto().to_owned()),
        ("impressao", estado.esta_maquina.impressao()),
        ("politica", politica_de(estado).to_owned()),
        ("enlace", estado.enlace.frase().to_owned()),
        ("borda", estado.borda_do_par.nome().to_owned()),
        (
            "portador",
            estado
                .portador
                .map_or("nenhum", Portador::nome_tecnico)
                .to_owned(),
        ),
        (
            "motivo",
            estado
                .motivo_do_portador
                .map_or("-", MotivoDoPortador::frase)
                .to_owned(),
        ),
        ("atraso", atraso_de(estado)),
        ("nivel", nivel_de(estado)),
        ("agente pronto", estado.agente_pronto.to_string()),
        ("bloqueio permitido", estado.bloqueio_permitido.to_string()),
        (
            "ultima queda",
            estado
                .ultima_queda
                .map_or("nenhuma", MotivoDaQueda::frase)
                .to_owned(),
        ),
    ];

    let mut linhas = Vec::with_capacity(campos.len() + 1);
    linhas.push("InputRemote - diagnostico (servico simulado)".to_owned());
    for (rotulo, valor) in campos {
        linhas.push(format!("{rotulo}: {valor}"));
    }
    linhas.join("\n")
}

const fn politica_de(estado: &Estado) -> &'static str {
    match estado.politica {
        Politica::Ambos => "ambos",
        Politica::SoEste => "so-este",
        Politica::SoOOutro => "so-o-outro",
    }
}

fn atraso_de(estado: &Estado) -> String {
    estado.latencia.map_or_else(
        || "sem amostras".to_owned(),
        |medida| {
            format!(
                "mediana {} ms, p99 {} ms, {} amostras",
                medida.mediana_ms, medida.p99_ms, medida.amostras
            )
        },
    )
}

/// O nível de capacidade por extenso, para o relatório valer sozinho.
///
/// "N2" não diz nada a quem lê um relato colado num registro de problema.
fn nivel_de(estado: &Estado) -> String {
    let nivel = estado.nivel_privilegiado;
    format!("{} ({})", nivel.rotulo(), nivel.explicacao())
}
