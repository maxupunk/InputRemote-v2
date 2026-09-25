//! Onde os canais locais moram: o de controle e o do agente.
//!
//! Um lugar só para o serviço, que escuta, e para a interface, o agente e a ferramenta de bancada,
//! que conectam. Cada lado interpretava as variáveis de sobrescrita por conta própria, e bastava um
//! deles divergir para procurar o serviço num lugar em que ele não está.
//!
//! A sobrescrita aceita caminho completo ou nome curto: um valor sem separador vira
//! `\\.\pipe\<nome>` no Windows e um socket no diretório temporário no Linux. O nome curto existe
//! porque a barra invertida do caminho de *pipe* não sobrevive a algumas camadas de shell.

/// A variável que sobrescreve o endereço do canal de controle, para o teste.
pub const VARIAVEL_DO_CONTROLE: &str = "IR_CONTROL_ENDPOINT";

/// A variável que sobrescreve o endereço do canal do agente, para o teste.
pub const VARIAVEL_DO_AGENTE: &str = "IR_AGENT_ENDPOINT";

/// O endereço padrão do canal de controle.
#[cfg(windows)]
const PADRAO_DO_CONTROLE: &str = r"\\.\pipe\inputremote-control";
#[cfg(not(windows))]
const PADRAO_DO_CONTROLE: &str = "/run/inputremote/control.sock";

/// O endereço padrão do canal do agente.
#[cfg(windows)]
const PADRAO_DO_AGENTE: &str = r"\\.\pipe\inputremote-agent";
#[cfg(not(windows))]
const PADRAO_DO_AGENTE: &str = "/run/inputremote/agent.sock";

/// O endereço do canal de controle, o da interface.
#[must_use]
pub fn do_controle() -> String {
    resolver(VARIAVEL_DO_CONTROLE, PADRAO_DO_CONTROLE)
}

/// O endereço do canal do agente, o que carrega injeção de entrada.
#[must_use]
pub fn do_agente() -> String {
    resolver(VARIAVEL_DO_AGENTE, PADRAO_DO_AGENTE)
}

/// A sobrescrita de `variavel`, quando há, ou o `padrao` da plataforma.
fn resolver(variavel: &str, padrao: &str) -> String {
    match std::env::var(variavel) {
        Ok(valor) if !valor.is_empty() => expandir(&valor),
        _ => padrao.to_owned(),
    }
}

/// Expande uma sobrescrita curta para um endereço completo da plataforma.
fn expandir(valor: &str) -> String {
    if valor.contains(['\\', '/']) {
        return valor.to_owned();
    }
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\{valor}")
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir()
            .join(format!("{valor}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn um_caminho_completo_nao_e_mexido() {
        let caminho = if cfg!(windows) {
            r"\\.\pipe\algum-nome"
        } else {
            "/tmp/algum.sock"
        };
        assert_eq!(expandir(caminho), caminho);
    }

    #[test]
    fn um_nome_curto_vira_um_endereco_da_plataforma() {
        let expandido = expandir("ir-teste");
        let esperado = if cfg!(windows) {
            r"\\.\pipe\ir-teste".to_owned()
        } else {
            std::env::temp_dir()
                .join("ir-teste.sock")
                .to_string_lossy()
                .into_owned()
        };
        assert_eq!(expandido, esperado);
    }

    #[test]
    fn sem_sobrescrita_vale_o_padrao_da_plataforma() {
        // Uma variável que ninguém define: o caminho do padrão, sem depender do ambiente do teste.
        let resolvido = resolver("IR_TESTE_VARIAVEL_QUE_NAO_EXISTE", PADRAO_DO_AGENTE);
        assert_eq!(resolvido, PADRAO_DO_AGENTE);
        if cfg!(windows) {
            assert_eq!(PADRAO_DO_CONTROLE, r"\\.\pipe\inputremote-control");
            assert_eq!(PADRAO_DO_AGENTE, r"\\.\pipe\inputremote-agent");
        } else {
            assert_eq!(PADRAO_DO_CONTROLE, "/run/inputremote/control.sock");
            assert_eq!(PADRAO_DO_AGENTE, "/run/inputremote/agent.sock");
        }
    }
}
