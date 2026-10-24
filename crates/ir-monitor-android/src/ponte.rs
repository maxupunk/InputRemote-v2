//! O contrato com o Kotlin: cada evento vira um objeto JSON com `tipo`.
//!
//! JSON, e não `postcard`: o aplicativo o lê com o `org.json` que já vem no Android, sem
//! dependência nenhuma, e a um evento por segundo o custo não aparece. Os nomes são o contrato —
//! mudar um deles quebra a tela de lá em silêncio, e os testes abaixo existem por isso.

use serde_json::{Value, json};

use crate::cliente::Evento;
use crate::cofre::Computador;
use crate::descoberta::Achado;

/// O evento em JSON.
#[must_use]
pub fn evento(evento: &Evento) -> String {
    let valor = match evento {
        Evento::Codigo(digitos) => json!({
            "tipo": "codigo",
            "digitos": digitos.iter().map(u8::to_string).collect::<String>(),
        }),
        Evento::Aberta { computador, pareou } => json!({
            "tipo": "aberta",
            "computador": computador,
            "pareou": pareou,
        }),
        Evento::Quadro(vista) => json!({
            "tipo": "quadro",
            "vista": serde_json::to_value(vista).unwrap_or(Value::Null),
        }),
        Evento::Caiu { motivo, recusado } => json!({
            "tipo": "caiu",
            "motivo": motivo,
            "recusado": recusado,
        }),
    };
    valor.to_string()
}

/// Os nomes de processos que o Kotlin manda, de um vetor JSON de textos. O que não for isso vira
/// lista vazia: a tela fica sem os cards de processo, e não sem o painel.
#[must_use]
pub fn nomes(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Os computadores que a descoberta achou, em JSON.
#[must_use]
pub fn achados(achados: &[Achado]) -> String {
    let lista: Vec<Value> = achados
        .iter()
        .map(|a| json!({ "nome": a.nome, "endereco": a.endereco.to_string() }))
        .collect();
    Value::Array(lista).to_string()
}

/// Os computadores pareados, em JSON.
#[must_use]
pub fn pareados(computadores: &[Computador]) -> String {
    let lista: Vec<Value> = computadores
        .iter()
        .map(|c| json!({ "nome": c.nome, "chave": c.chave, "endereco": c.endereco }))
        .collect();
    Value::Array(lista).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir_monitor::{Maquina, Quadro, Situacao, Vista};

    fn ler(texto: &str) -> Value {
        serde_json::from_str(texto).unwrap()
    }

    #[test]
    fn o_codigo_vai_como_seis_digitos_em_texto() {
        let valor = ler(&evento(&Evento::Codigo([0, 4, 1, 9, 0, 7])));
        assert_eq!(valor["tipo"], "codigo");
        assert_eq!(valor["digitos"], "041907");
    }

    #[test]
    fn o_quadro_leva_a_vista_inteira() {
        let quadro = Quadro {
            maquinas: vec![Maquina {
                nome: "DESKTOP".to_owned(),
                situacao: Situacao::Aguardando,
            }],
            enlace: None,
        };
        let valor = ler(&evento(&Evento::Quadro(Vista::do_quadro(&quadro))));
        assert_eq!(valor["tipo"], "quadro");
        assert_eq!(valor["vista"]["maquinas"][0]["nome"], "DESKTOP");
        assert!(valor["vista"]["maquinas"][0]["aviso"].is_string());
        assert!(valor["vista"]["enlace"].is_null());
    }

    #[test]
    fn os_detalhes_vao_dentro_da_vista() {
        use ir_monitor::{Detalhes, Processos, UsoDeProcesso};
        let quadro = Quadro {
            maquinas: vec![Maquina {
                nome: "DESKTOP".to_owned(),
                situacao: Situacao::Amostra(ir_proto::message::UsageSample::default()),
            }],
            enlace: None,
        };
        let docker = UsoDeProcesso {
            nome: "docker".to_owned(),
            instancias: 2,
            cpu_permille: 15,
            memoria_kib: 2048,
        };
        let detalhes = Detalhes {
            copia: None,
            processos: vec![Some(Processos {
                vigiados: vec![docker.clone()],
                lista: vec![docker],
            })],
        };
        let vista = Vista::montar(&quadro, Some(&detalhes));
        let valor = ler(&evento(&Evento::Quadro(vista)));
        let maquina = &valor["vista"]["maquinas"][0];
        let card = maquina["blocos"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["chave"] == "proc:docker");
        assert!(card.is_some(), "o card do processo vai entre os blocos");
        let linha = &maquina["processos"][0];
        assert_eq!(linha["nome"], "docker");
        assert_eq!(linha["cpu_permille"], 15);
        assert_eq!(linha["memoria_kib"], 2048);
        assert_eq!(linha["chave"], "proc:docker");
        assert!(valor["vista"]["copia"].is_null());
    }

    #[test]
    fn os_nomes_vem_de_um_vetor_json() {
        assert_eq!(nomes(r#"["docker","wsl"]"#), vec!["docker", "wsl"]);
        assert!(nomes("lixo").is_empty());
    }

    #[test]
    fn a_queda_diz_se_foi_recusa() {
        let valor = ler(&evento(&Evento::Caiu {
            motivo: "x".to_owned(),
            recusado: true,
        }));
        assert_eq!(valor["tipo"], "caiu");
        assert_eq!(valor["recusado"], true);
    }

    #[test]
    fn as_listas_sao_vetores_de_objetos() {
        let achado = Achado {
            nome: "PC".to_owned(),
            maquina: String::new(),
            endereco: "10.0.0.2".parse().unwrap(),
        };
        assert_eq!(ler(&achados(&[achado]))[0]["endereco"], "10.0.0.2");
        assert_eq!(ler(&pareados(&[]))[0], Value::Null);
    }
}
