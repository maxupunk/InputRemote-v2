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

/// Traduz o estado publicado para o que a janela desenha.
#[must_use]
pub fn estado_ui(estado: &Estado) -> EstadoUi {
    EstadoUi {
        resumo: estado.resumo().into(),
        enlace: estado.enlace.frase().into(),
        saude: saude(estado),
        conectado: estado.enlace.conectado(),
        servidor: estado.papel == Papel::Servidor,
        borda: indice_da_borda(estado.borda_do_par),
        tem_par: estado.par.is_some(),
        par_nome: nome_do_par(estado),
        este_nome: estado.este_nome.como_texto().into(),
        esta_impressao: estado.esta_maquina.impressao().into(),
        portador: estado.portador.map_or("", Portador::nome).into(),
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
    }
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

#[cfg(test)]
mod tests {
    use ir_ipc::status::{Latencia, MotivoDaQueda};
    use ir_ipc::vocabulario::{Maquina, Nivel, Nome};

    use super::*;

    fn recem_instalado() -> Estado {
        Estado::recem_instalado(Maquina([0xAB; 16]), Nome::coagido("bancada"))
    }

    #[test]
    fn a_primeira_tela_diz_o_primeiro_passo() {
        let ui = estado_ui(&recem_instalado());
        assert!(ui.resumo.contains("Pareie"), "{}", ui.resumo);
        assert!(!ui.tem_par);
        assert!(!ui.conectado);
    }

    #[test]
    fn sem_o_agente_nada_mais_importa_para_a_cor() {
        // Mesmo com sessão de pé, se o componente que digita não subiu o produto não funciona.
        let mut estado = recem_instalado();
        estado.enlace = LinkState::EmUso;
        estado.agente_pronto = false;
        assert_eq!(saude(&estado), SAUDE_RUIM);
    }

    #[test]
    fn encerrar_por_vontade_do_usuario_nao_pinta_de_vermelho() {
        // Pintar de vermelho o que o usuário acabou de pedir ensina a ignorar o vermelho — e aí a
        // queda que importa passa batida.
        let mut estado = recem_instalado();
        estado.agente_pronto = true;
        estado.ultima_queda = Some(MotivoDaQueda::PedidoPeloUsuario);
        assert_eq!(saude(&estado), SAUDE_ATENCAO);

        estado.ultima_queda = Some(MotivoDaQueda::MeioFalhou);
        assert_eq!(saude(&estado), SAUDE_RUIM);
    }

    #[test]
    fn conectando_tem_saude_de_andamento() {
        let mut estado = recem_instalado();
        estado.agente_pronto = true;
        estado.enlace = LinkState::Conectando;
        assert_eq!(saude(&estado), SAUDE_ANDAMENTO);
    }

    #[test]
    fn as_quatro_bordas_sobrevivem_a_ida_e_volta() {
        for borda in Borda::TODAS {
            assert_eq!(borda_do_indice(indice_da_borda(borda)), borda);
        }
    }

    #[test]
    fn indice_de_borda_invalido_nao_derruba_a_janela() {
        for indice in [-7, 4, i32::MAX, i32::MIN] {
            assert_eq!(borda_do_indice(indice), Borda::Direita);
        }
    }

    #[test]
    fn as_tres_opcoes_de_conexao_sobrevivem_a_ida_e_volta() {
        for escolha in [None, Some(Portador::Bluetooth), Some(Portador::RedeLocal)] {
            assert_eq!(portador_do_indice(indice_do_portador(escolha)), escolha);
        }
    }

    #[test]
    fn a_rede_de_arquivos_nao_e_oferecida_como_meio_de_teclado() {
        // Ela não carrega entrada, então não pode ser escolhível. Se o estado disser que é ela, a
        // tela mostra "Rede" — mas escolher "Rede" nunca produz a rede de arquivos.
        assert_eq!(indice_do_portador(Some(Portador::RedeDeArquivos)), 2);
        assert_eq!(portador_do_indice(2), Some(Portador::RedeLocal));
        assert!(!Portador::RedeDeArquivos.serve_para_entrada());
    }

    #[test]
    fn sem_medida_a_latencia_nao_e_acusada_de_ruim() {
        let estado = recem_instalado();
        assert!(latencia_boa(&estado));
        assert_eq!(estado_ui(&estado).latencia, "");
    }

    #[test]
    fn a_latencia_e_julgada_contra_a_meta_do_portador_em_uso() {
        let mut estado = recem_instalado();
        estado.latencia = Some(Latencia {
            mediana_ms: 15,
            p99_ms: 40,
            amostras: 400,
        });

        estado.portador = Some(Portador::Bluetooth);
        assert!(
            latencia_boa(&estado),
            "15 ms está dentro da meta do Bluetooth"
        );

        estado.portador = Some(Portador::RedeLocal);
        assert!(
            !latencia_boa(&estado),
            "15 ms na rede local está fora da meta"
        );
    }

    #[test]
    fn o_impedimento_vira_texto_vazio_quando_nao_ha_nenhum() {
        // A tela decide mostrar a faixa comparando com "". Se o "nenhum" viesse como a palavra
        // "None", a faixa apareceria sempre.
        let mut estado = recem_instalado();
        estado.agente_pronto = true;
        assert_eq!(estado_ui(&estado).impedimento, "");
    }

    #[test]
    fn a_tela_recebe_a_impressao_agrupada_e_o_nivel_por_extenso() {
        let mut estado = recem_instalado();
        estado.nivel_privilegiado = Nivel::TelaDeBloqueio;
        let ui = estado_ui(&estado);

        assert_eq!(ui.nivel, "N2");
        assert!(ui.nivel_suficiente);
        assert!(
            !ui.nivel_explicacao.is_empty(),
            "um selo sem explicação não informa nada"
        );
        assert_eq!(
            ui.esta_impressao.split(' ').count(),
            8,
            "{}",
            ui.esta_impressao
        );
    }
}
