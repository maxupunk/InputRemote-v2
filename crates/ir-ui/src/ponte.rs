//! A tradução de [`ir_ipc::Estado`] para o que a tela desenha.
//!
//! Toda frase que o usuário lê nasce aqui ou em `ir-ipc`, e nunca em expressões espalhadas pelos
//! arquivos `.slint`. O motivo é simples: uma frase errada precisa ter um lugar só onde ser
//! consertada, e precisa ter teste. Expressão em arquivo de layout não tem nem uma coisa nem outra.
//!
//! O que este módulo faz de verdade é converter conceitos em índices. O Slint não tem enumerações
//! vindas do Rust sem custo de manutenção, então borda e portador viajam como `int` — e a conversão
//! nos dois sentidos fica aqui, com teste de ida e volta para cada valor.

use ir_ipc::status::{Estado, LinkState, Papel};
use ir_ipc::vocabulario::{Borda, Portador};
use slint::SharedString;

use crate::gerado::EstadoUi;

/// Tudo vai bem.
pub const SAUDE_BOA: i32 = 0;
/// Algo está em andamento.
pub const SAUDE_ANDAMENTO: i32 = 1;
/// Há algo a resolver, mas o produto não está quebrado.
pub const SAUDE_ATENCAO: i32 = 2;
/// O produto não funciona agora.
pub const SAUDE_RUIM: i32 = 3;

/// O que dizer quando o outro computador está na tela de bloqueio e recusa o que se digita daqui.
///
/// Sem isto o teclado simplesmente parava de funcionar lá, e a pessoa não tinha como saber se era
/// defeito ou proteção.
pub const AVISO_DO_BLOQUEIO_DO_PAR: &str = "O outro computador está na tela de bloqueio, e o que \
     você digita daqui não chega lá: a digitação na tela de bloqueio não foi permitida nele. Um \
     administrador de lá pode ligar em Preferências.";

/// Traduz o estado publicado para o que a janela desenha.
#[must_use]
pub fn estado_ui(estado: &Estado) -> EstadoUi {
    EstadoUi {
        resumo: estado.resumo().into(),
        enlace: estado.frase_do_enlace().into(),
        saude: saude(estado),
        conectado: estado.enlace.conectado(),
        servidor: estado.papel == Papel::Servidor,
        borda: indice_da_borda(estado.borda_do_par),
        tem_par: estado.par.is_some(),
        par_nome: nome_do_par(estado),
        este_nome: estado.este_nome.como_texto().into(),
        esta_impressao: estado.esta_maquina.impressao().into(),
        portador: estado.nome_da_rota().into(),
        motivo_do_portador: estado
            .motivo_do_portador
            .map_or_else(SharedString::default, |motivo| motivo.frase().into()),
        portador_fixado: indice_do_portador(estado.portador_fixado),
        latencia: texto_da_latencia(estado),
        latencia_boa: latencia_boa(estado),
        nivel: estado.nivel_privilegiado.rotulo().into(),
        nivel_explicacao: estado.nivel_privilegiado.explicacao().into(),
        nivel_suficiente: estado.nivel_privilegiado.suficiente(),
        bloqueio_permitido: estado.bloqueio_permitido,
        impedimento: estado.impedimento().unwrap_or_default().into(),
        aviso_de_rede: estado
            .aviso_de_rede()
            .map_or_else(SharedString::default, |aviso| aviso.frase.into()),
        aviso_de_rede_no_par: estado.aviso_de_rede().is_some_and(|aviso| aviso.no_par),
        pausado: estado.pausa == Some(ir_ipc::Pausa::Aqui),
        pausado_no_par: estado.pausa == Some(ir_ipc::Pausa::NoPar),
        aviso_do_bloqueio_do_par: if estado.par_recusa_tela_de_bloqueio {
            AVISO_DO_BLOQUEIO_DO_PAR.into()
        } else {
            SharedString::default()
        },
        sobre_o_bloqueio: estado.sobre_a_tela_de_bloqueio().into(),
        borda_travada: estado.borda_travada,
        bloquear_juntos: estado.bloquear_juntos,
    }
}

/// A dica do ícone da bandeja: o estado curto e a frase principal.
///
/// O Windows corta a dica em 127 caracteres; a frase principal é cortada antes, numa palavra.
#[must_use]
pub fn dica_da_bandeja(enlace: &str, resumo: &str) -> String {
    const TETO: usize = 120;
    let mut dica = format!("InputRemote — {enlace}");
    if !resumo.is_empty() {
        dica.push('\n');
        dica.push_str(resumo);
    }
    if dica.chars().count() > TETO {
        let cortada: String = dica.chars().take(TETO - 1).collect();
        let ate_palavra = cortada
            .rfind(' ')
            .map_or(cortada.as_str(), |fim| &cortada[..fim]);
        return format!("{ate_palavra}…");
    }
    dica
}

/// Quão bem o produto está, num número que governa cor e ponto.
///
/// A escala não é a do enlace: um enlace desconectado porque o usuário mandou parar não é um
/// problema, e um enlace desconectado porque o rádio caiu é. Pintar os dois de vermelho ensina o
/// usuário a ignorar vermelho.
#[must_use]
pub fn saude(estado: &Estado) -> i32 {
    if !estado.agente_pronto {
        return SAUDE_RUIM;
    }
    // Uma pausa é vontade de alguém, e não defeito: nem vermelho, nem verde.
    if estado.pausa.is_some() {
        return SAUDE_ATENCAO;
    }
    match estado.enlace {
        LinkState::Pronto | LinkState::EmUso => {
            if estado.impedimento().is_some() {
                SAUDE_ATENCAO
            } else {
                SAUDE_BOA
            }
        }
        LinkState::Conectando => SAUDE_ANDAMENTO,
        LinkState::Desconectado => saude_desconectado(estado),
        // Um estado de enlace que esta versão da interface não conhece é motivo de atenção, e não
        // de alarme: o serviço ser mais novo que a janela é normal durante uma atualização.
        _ => SAUDE_ATENCAO,
    }
}

fn saude_desconectado(estado: &Estado) -> i32 {
    use ir_ipc::status::MotivoDaQueda as Q;
    // Só três motivos são falha de verdade. Uma queda que o usuário provocou, um par que se
    // suspendeu, um serviço que está parando e uma troca de meio são comportamento normal — pintar
    // de vermelho o que é normal ensina o usuário a ignorar vermelho, e aí a falha que importa
    // passa batida.
    match estado.ultima_queda {
        Some(Q::MeioFalhou | Q::ParNaoRespondeu | Q::ErroDeProtocolo) => SAUDE_RUIM,
        _ => SAUDE_ATENCAO,
    }
}

fn nome_do_par(estado: &Estado) -> SharedString {
    estado
        .par
        .as_ref()
        .map_or_else(SharedString::default, |par| par.nome.como_texto().into())
}

fn texto_da_latencia(estado: &Estado) -> SharedString {
    estado
        .latencia
        .map_or_else(SharedString::default, |medida| {
            // Mediana e p99 juntos, porque só a mediana esconde o que o usuário sente: 8 ms de mediana
            // com 120 ms de pior caso é pior de usar que 20 ms constantes.
            format!(
                "{} ms · {} ms no pior caso",
                medida.mediana_ms, medida.p99_ms
            )
            .into()
        })
}

fn latencia_boa(estado: &Estado) -> bool {
    match (estado.latencia, estado.portador) {
        (Some(medida), Some(portador)) => medida.dentro_da_meta(portador),
        // Nenhuma amostra ainda não é o mesmo que atraso alto. Pintar de laranja o que não foi
        // medido é inventar um problema que não existe.
        _ => true,
    }
}

/// O índice com que a tela representa uma borda.
#[must_use]
pub const fn indice_da_borda(borda: Borda) -> i32 {
    match borda {
        Borda::Esquerda => 0,
        Borda::Direita => 1,
        Borda::Acima => 2,
        Borda::Abaixo => 3,
    }
}

/// A borda que um índice da tela representa.
///
/// Índice desconhecido vira [`Borda::Direita`] em vez de erro: o valor vem da nossa própria tela, e
/// derrubar a interface por um número que só ela produz seria trocar um defeito de layout por uma
/// janela que fecha.
#[must_use]
pub const fn borda_do_indice(indice: i32) -> Borda {
    match indice {
        0 => Borda::Esquerda,
        2 => Borda::Acima,
        3 => Borda::Abaixo,
        _ => Borda::Direita,
    }
}

/// O índice com que a tela representa a preferência de portador.
#[must_use]
pub const fn indice_do_portador(portador: Option<Portador>) -> i32 {
    match portador {
        None => 0,
        Some(Portador::Bluetooth) => 1,
        Some(Portador::RedeLocal | Portador::RedeDeArquivos) => 2,
    }
}

/// A preferência de portador que um índice da tela representa.
///
/// A tela oferece três opções e o produto tem três portadores, mas não são os mesmos três: a rede
/// de arquivos não carrega teclado e mouse, então "Rede" na interface quer dizer
/// [`Portador::RedeLocal`].
#[must_use]
pub const fn portador_do_indice(indice: i32) -> Option<Portador> {
    match indice {
        1 => Some(Portador::Bluetooth),
        2 => Some(Portador::RedeLocal),
        _ => None,
    }
}

/// A porta em que o serviço escuta quando ninguém muda (`docs/03-protocolo.md` §10).
const PORTA_PADRAO: u16 = 52525;

/// O endereço que a pessoa digitou, no formato que o serviço entende — ou por que não serve.
///
/// Aceita o IP sozinho (a porta é a padrão), `ip:porta`, e o endereço Bluetooth (`AA:BB:CC:DD:EE:FF`).
/// Nome de máquina não: resolver nome exigiria DNS, e numa rede sem ele o erro seria mudo.
///
/// # Errors
///
/// A frase que a tela mostra embaixo do campo.
pub fn ler_endereco_digitado(texto: &str) -> Result<String, &'static str> {
    let texto = texto.trim();
    if texto.is_empty() {
        return Err("Digite o endereço do outro computador.");
    }
    if let Ok(endereco) = texto.parse::<std::net::SocketAddr>() {
        return Ok(endereco.to_string());
    }
    if let Ok(ip) = texto.parse::<std::net::IpAddr>() {
        return Ok(std::net::SocketAddr::new(ip, PORTA_PADRAO).to_string());
    }
    if e_bluetooth(texto) {
        return Ok(texto.to_ascii_uppercase());
    }
    Err(
        "Esse endereço não foi reconhecido. Use o IP do outro computador, como 192.168.0.10, \
         ou o endereço Bluetooth dele, como AC:50:DE:47:EB:28.",
    )
}

/// Seis pares hexadecimais separados por dois-pontos.
fn e_bluetooth(texto: &str) -> bool {
    let partes: Vec<&str> = texto.split(':').collect();
    partes.len() == 6
        && partes
            .iter()
            .all(|parte| parte.len() == 2 && parte.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests_do_endereco {
    use super::*;

    #[test]
    fn a_explicacao_do_endereco_invalido_nao_tem_espaco_sobrando() {
        let explicacao = ler_endereco_digitado("isto não é endereço").unwrap_err();
        assert!(!explicacao.contains("  "), "{explicacao}");
    }

    #[test]
    fn o_ip_sozinho_ganha_a_porta_padrao() {
        assert_eq!(
            ler_endereco_digitado(" 10.0.0.135 ").unwrap(),
            "10.0.0.135:52525"
        );
    }

    #[test]
    fn a_porta_digitada_e_respeitada() {
        assert_eq!(
            ler_endereco_digitado("10.0.0.135:52526").unwrap(),
            "10.0.0.135:52526"
        );
    }

    #[test]
    fn o_endereco_bluetooth_serve_em_qualquer_caixa() {
        assert_eq!(
            ler_endereco_digitado("ac:50:de:47:eb:28").unwrap(),
            "AC:50:DE:47:EB:28"
        );
    }

    #[test]
    fn o_que_nao_e_endereco_volta_com_o_motivo() {
        for lixo in [
            "",
            "   ",
            "fedora.local",
            "10.0.0",
            "AC:50:DE:47:EB",
            "10.0.0.1:porta",
        ] {
            let motivo = ler_endereco_digitado(lixo).unwrap_err();
            assert!(motivo.len() > 20, "{lixo:?}: {motivo}");
        }
    }
}

#[cfg(test)]
mod testes;
