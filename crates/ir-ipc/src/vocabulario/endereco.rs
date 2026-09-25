//! O endereço do outro computador, como a pessoa o digita.
//!
//! Não confundir com [`crate::endereco`], que é onde moram os canais **locais** do serviço. Este é o
//! do par: um IP na rede ou um endereço de rádio Bluetooth.

use crate::Falha;

/// A porta em que o serviço escuta quando ninguém muda — a do protocolo, [`ir_proto::DEFAULT_PORT`].
pub const PORTA_PADRAO: u16 = ir_proto::DEFAULT_PORT;

/// O endereço digitado, no formato que o serviço entende.
///
/// Aceita o IP sozinho (a porta é a padrão), `ip:porta`, e o endereço Bluetooth
/// (`AA:BB:CC:DD:EE:FF`, em qualquer caixa). Nome de máquina não: resolver nome exigiria DNS, e numa
/// rede sem ele o erro seria mudo. A instrução de [`Falha::EnderecoInvalido`] descreve exatamente
/// estas formas, e é por isso que as duas moram no mesmo crate.
///
/// # Errors
///
/// [`Falha::EnderecoInvalido`] quando o texto não é nenhuma das formas.
pub fn normalizar(texto: &str) -> Result<String, Falha> {
    let texto = texto.trim();
    if let Ok(endereco) = texto.parse::<std::net::SocketAddr>() {
        return Ok(endereco.to_string());
    }
    if let Ok(ip) = texto.parse::<std::net::IpAddr>() {
        return Ok(std::net::SocketAddr::new(ip, PORTA_PADRAO).to_string());
    }
    if e_bluetooth(texto) {
        return Ok(texto.to_ascii_uppercase());
    }
    Err(Falha::EnderecoInvalido)
}

/// Seis pares hexadecimais separados por dois-pontos.
fn e_bluetooth(texto: &str) -> bool {
    let mut partes = 0;
    for parte in texto.split(':') {
        partes += 1;
        if parte.len() != 2 || !parte.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }
    partes == 6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_ip_sozinho_ganha_a_porta_padrao() {
        assert_eq!(normalizar(" 10.0.0.135 ").unwrap(), "10.0.0.135:52525");
    }

    #[test]
    fn a_porta_digitada_e_respeitada() {
        assert_eq!(normalizar("10.0.0.135:52526").unwrap(), "10.0.0.135:52526");
    }

    #[test]
    fn o_endereco_bluetooth_serve_em_qualquer_caixa() {
        assert_eq!(
            normalizar("ac:50:de:47:eb:28").unwrap(),
            "AC:50:DE:47:EB:28"
        );
    }

    #[test]
    fn o_que_nao_e_endereco_e_recusado() {
        for lixo in [
            "",
            "   ",
            "fedora.local",
            "10.0.0",
            "AC:50:DE:47:EB",
            "AC:50:DE:47:EB:28:11",
            "10.0.0.1:porta",
        ] {
            assert_eq!(normalizar(lixo), Err(Falha::EnderecoInvalido), "{lixo:?}");
        }
    }

    #[test]
    fn a_instrucao_do_erro_aceita_o_que_o_normalizador_aceita() {
        // O defeito que isto trava: a instrução pedia `192.168.0.10:52525`, a janela dizia
        // `192.168.0.10`, e as duas formas eram aceitas. Os exemplos da instrução têm de passar.
        let instrucao = Falha::EnderecoInvalido.o_que_fazer();
        assert!(instrucao.contains("192.168.0.10"), "{instrucao}");
        assert!(normalizar("192.168.0.10").is_ok());
        assert!(instrucao.contains("AC:50:DE:47:EB:28"), "{instrucao}");
        assert!(normalizar("AC:50:DE:47:EB:28").is_ok());
    }
}
