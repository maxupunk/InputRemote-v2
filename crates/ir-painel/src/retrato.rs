//! O retrato do serviço, e o [`Estado`] que a janela desenha a partir dele.

use ir_ipc::{
    EconomiaDoWifi, Estado, Latencia, Maquina, MotivoDaQueda, MotivoDoPortador, Nivel, Nome,
    ParConhecido, Pausa, Portador, Recursos,
};
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::{PeerInfo, Phase, Policy};

use crate::traducao::{borda_de, link_state, politica_de, portador_de};

/// O par gravado, e o que se sabe dele agora.
#[derive(Debug, Clone, Copy)]
pub struct ParGravado<'a> {
    /// A máquina, pela chave fixada.
    pub maquina: Maquina,
    /// O nome que ele deu a si mesmo na última sessão, gravado.
    pub nome_gravado: Option<&'a str>,
    /// O que ele disse nesta sessão, se há sessão.
    pub ao_vivo: Option<&'a PeerInfo>,
    /// Se há enlace seguro de pé com ele.
    pub conectado: bool,
}

/// O que esta máquina tem para digitar e capturar.
#[derive(Debug, Clone, Copy)]
pub struct Entrada<'a> {
    /// Se o agente (Windows) está conectado e pronto.
    pub agente_pronto: bool,
    /// Se o serviço injeta direto — o `uinput` do Linux.
    pub injeta_direto: bool,
    /// Se o serviço captura direto — o `evdev` do Linux.
    pub captura_direto: bool,
    /// Os desktops em que o agente injeta: `Winlogon` é a tela de bloqueio.
    pub desktops_do_agente: &'a [String],
}

/// Tudo que o serviço sabe e a janela precisa, num valor só.
///
/// Os booleanos são fatos independentes que a janela mostra lado a lado, e não estados de uma
/// máquina: juntá-los num enum inventaria combinações que não existem.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct Retrato<'a> {
    /// A fase da sessão.
    pub fase: Phase,
    /// Quem pode controlar quem.
    pub politica: Policy,
    /// A borda que dá para o par.
    pub borda: Edge,
    /// Esta máquina.
    pub maquina: Maquina,
    /// O nome desta máquina.
    pub nome: &'a Nome,
    /// O par gravado, se há.
    pub par: Option<ParGravado<'a>>,
    /// O portador que a sessão usa agora.
    pub portador: Option<Carrier>,
    /// O portador fixado nas preferências.
    pub portador_fixado: Option<Portador>,
    /// Se a rota é dupla: Bluetooth e rede ao mesmo tempo.
    pub rota_dupla: bool,
    /// A latência da última janela.
    pub latencia: Option<Latencia>,
    /// O que esta máquina tem para digitar e capturar.
    pub entrada: Entrada<'a>,
    /// Se o par pode digitar na tela de bloqueio daqui.
    pub bloqueio_permitido: bool,
    /// Por que a última sessão caiu.
    pub ultima_queda: Option<MotivoDaQueda>,
    /// Quanto a pasta de recebidos ocupa.
    pub recebidos_bytes: u64,
    /// A economia do Wi-Fi daqui, quando atrapalha.
    pub economia_aqui: Option<EconomiaDoWifi>,
    /// A do par, quando atrapalha.
    pub economia_no_par: Option<EconomiaDoWifi>,
    /// Se está pausado, e de que lado.
    pub pausa: Option<Pausa>,
    /// Se o par disse que recusa digitação daqui no desktop protegido dele.
    pub par_recusa_protegido: bool,
    /// Onde os recebidos ficam.
    pub pasta_de_recebidos: String,
    /// Se a borda está travada.
    pub borda_travada: bool,
    /// Se o par bloqueia junto.
    pub bloquear_juntos: bool,
}

/// O estado, no vocabulário publicado da interface.
#[must_use]
pub fn estado(r: &Retrato<'_>) -> Estado {
    let estabelecida = r.fase.is_established();
    Estado {
        enlace: link_state(r.fase),
        politica: politica_de(r.politica),
        borda_do_par: borda_de(r.borda),
        esta_maquina: r.maquina,
        este_nome: r.nome.clone(),
        par: r.par.map(|par| par_conhecido(&par)),
        portador: r.portador.map(portador_de),
        portador_fixado: r.portador_fixado,
        motivo_do_portador: motivo_do_portador(r),
        latencia: r.latencia.filter(|_| estabelecida),
        nivel_privilegiado: nivel(&r.entrada),
        // Receber: ou o agente está de pé (Windows), ou o serviço injeta direto (Linux).
        agente_pronto: r.entrada.agente_pronto || r.entrada.injeta_direto,
        bloqueio_permitido: r.bloqueio_permitido,
        ultima_queda: r.ultima_queda,
        recebidos_bytes: r.recebidos_bytes,
        rota_dupla: r.rota_dupla,
        economia_aqui: r.economia_aqui,
        economia_no_par: r.economia_no_par,
        pausa: r.pausa,
        // O que o par disse vale para a sessão dele; sem sessão, não vale mais.
        par_recusa_tela_de_bloqueio: r.par_recusa_protegido && estabelecida,
        pasta_de_recebidos: r.pasta_de_recebidos.clone(),
        borda_travada: r.borda_travada,
        bloquear_juntos: r.bloquear_juntos,
        // Ler o teclado daqui: no Windows é o agente que captura; no Linux, o serviço.
        captura_pronta: r.entrada.agente_pronto || r.entrada.captura_direto,
    }
}

/// Até onde esta máquina consegue receber digitação sem sessão desbloqueada.
///
/// Era `SoDesbloqueado` fixo, e a tela acusava "N1" em toda máquina. No Linux o serviço injeta
/// por `uinput`, que chega ao greeter e à tela de bloqueio: N3. No Windows depende do agente — se
/// ele abriu o desktop `Winlogon`, a tela de bloqueio e a de login são alcançáveis, porque o serviço
/// o lança na sessão de console mesmo antes de alguém entrar (ADR-0008).
#[must_use]
pub fn nivel(entrada: &Entrada<'_>) -> Nivel {
    if cfg!(target_os = "linux") && entrada.injeta_direto {
        return Nivel::TelaDeLogin;
    }
    if !entrada.agente_pronto {
        return if entrada.injeta_direto {
            Nivel::SoDesbloqueado
        } else {
            Nivel::Nenhum
        };
    }
    if entrada
        .desktops_do_agente
        .iter()
        .any(|desktop| desktop.eq_ignore_ascii_case("Winlogon"))
    {
        Nivel::TelaDeLogin
    } else {
        Nivel::SoDesbloqueado
    }
}

/// Por que o portador em uso foi escolhido.
///
/// Antes isto era `RedeComoAlternativa` fixo, o que fazia a tela dizer "Bluetooth indisponível;
/// usando a rede local" **mesmo com o rádio ligado e conectado dos dois lados**. A escolha é da
/// sessão; aqui só se conta qual foi.
fn motivo_do_portador(r: &Retrato<'_>) -> Option<MotivoDoPortador> {
    let portador = r.portador?;
    Some(if r.portador_fixado.is_some() {
        MotivoDoPortador::FixadoPeloUsuario
    } else if r.rota_dupla {
        MotivoDoPortador::Redundancia
    } else if portador == Carrier::Rfcomm {
        MotivoDoPortador::Preferido
    } else {
        MotivoDoPortador::RedeComoAlternativa
    })
}

/// O par gravado, resumido para a interface.
///
/// O nome e os recursos vêm do que ele disse: nesta sessão, ou gravado da última, para a tela dizer
/// quem é mesmo com ele desligado. Antes era "computador pareado" fixo — "Conectado a computador
/// pareado" parecia defeito.
fn par_conhecido(par: &ParGravado<'_>) -> ParConhecido {
    let nome = par
        .ao_vivo
        .map(|vivo| vivo.name.as_str())
        .or(par.nome_gravado)
        .unwrap_or("o outro computador");
    ParConhecido {
        maquina: par.maquina,
        nome: Nome::coagido(nome),
        recursos: par
            .ao_vivo
            .map_or_else(Recursos::default, |vivo| Recursos::from(vivo.capabilities)),
        conectado: par.conectado,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entrada(agente: bool, direto: bool, desktops: &[String]) -> Entrada<'_> {
        Entrada {
            agente_pronto: agente,
            injeta_direto: direto,
            captura_direto: false,
            desktops_do_agente: desktops,
        }
    }

    #[test]
    fn o_agente_no_winlogon_alcanca_a_tela_de_login() {
        let com = vec!["Default".to_owned(), "Winlogon".to_owned()];
        let sem = vec!["Default".to_owned()];
        assert_eq!(nivel(&entrada(true, false, &com)), Nivel::TelaDeLogin);
        assert_eq!(nivel(&entrada(true, false, &sem)), Nivel::SoDesbloqueado);
        assert_eq!(nivel(&entrada(false, false, &[])), Nivel::Nenhum);
    }

    #[test]
    fn o_par_sem_sessao_aparece_pelo_nome_gravado() {
        let par = ParGravado {
            maquina: Maquina([1; 16]),
            nome_gravado: Some("notebook-da-ana"),
            ao_vivo: None,
            conectado: false,
        };
        assert_eq!(par_conhecido(&par).nome.como_texto(), "notebook-da-ana");
        let sem_nome = ParGravado {
            nome_gravado: None,
            ..par
        };
        assert_eq!(
            par_conhecido(&sem_nome).nome.como_texto(),
            "o outro computador"
        );
    }
}
