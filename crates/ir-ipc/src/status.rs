//! O que a interface mostra.
//!
//! Este é o tipo que existe para cumprir [`01, §5`]: *"em qualquer momento, o estado corrente
//! DEVE ser observável: portador ativo, por que ele foi escolhido, latência mediana e p99
//! medidas na última janela de 10 s, e a última razão de queda."*
//!
//! Foi a ausência exatamente disso que tornou o v1 impossível de diagnosticar. A pergunta que
//! todo campo aqui responde é "por que não está funcionando?", e nenhum deles é decorativo.
//!
//! [`01, §5`]: ../../../docs/01-visao-e-escopo.md

use serde::{Deserialize, Serialize};

use crate::vocabulario::{Borda, Maquina, Nivel, Nome, Portador, Recursos};

/// Em que ponto a sessão está, na linguagem da interface.
///
/// **Não** é `ir_session::Phase`. A interface não conhece o produto
/// ([ADR-0007](../../../docs/adr/0007-ui-slint-processo-separado.md)), e o IPC é um contrato
/// publicado, não um vazamento de estado interno. O serviço traduz de um para o outro — e essa
/// tradução é justamente a fronteira que impede a interface de virar o produto, como aconteceu
/// no v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LinkState {
    /// Nenhum meio de conexão disponível.
    #[default]
    Desconectado,
    /// Um meio subiu; as duas máquinas estão se reconhecendo.
    Conectando,
    /// Pronto, com o controle nesta máquina.
    Pronto,
    /// O controle está do outro lado.
    EmUso,
}

impl LinkState {
    /// A frase que a interface mostra.
    #[must_use]
    pub const fn frase(self) -> &'static str {
        match self {
            Self::Desconectado => "Desconectado",
            Self::Conectando => "Conectando…",
            Self::Pronto => "Pronto",
            Self::EmUso => "Controlando o outro computador",
        }
    }

    /// Se há sessão de pé.
    #[must_use]
    pub const fn conectado(self) -> bool {
        matches!(self, Self::Pronto | Self::EmUso)
    }
}

/// O papel desta máquina.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Papel {
    /// Tem o teclado e o mouse.
    #[default]
    Servidor,
    /// É controlada.
    Cliente,
}

impl Papel {
    /// A frase que a interface mostra.
    #[must_use]
    pub const fn frase(self) -> &'static str {
        match self {
            Self::Servidor => "Este computador tem o teclado e o mouse",
            Self::Cliente => "Este computador é controlado pelo outro",
        }
    }
}

/// Por que o portador em uso foi escolhido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MotivoDoPortador {
    /// Bluetooth estava disponível e é o preferido para teclado e mouse.
    Preferido,
    /// Bluetooth não estava disponível; usando a rede local.
    RedeComoAlternativa,
    /// Fixado nas preferências.
    FixadoPeloUsuario,
}

impl MotivoDoPortador {
    /// A frase que a interface mostra.
    #[must_use]
    pub const fn frase(self) -> &'static str {
        match self {
            Self::Preferido => "Bluetooth disponível, que dá a latência mais constante",
            Self::RedeComoAlternativa => "Bluetooth indisponível; usando a rede local",
            Self::FixadoPeloUsuario => "Fixado nas preferências",
        }
    }
}

/// Latência medida na última janela.
///
/// Mediana **e** p99, porque só a mediana esconde o que o usuário sente: um ponteiro com
/// mediana de 8 ms e p99 de 120 ms é pior de usar que um com 20 ms constantes
/// (`docs/01-visao-e-escopo.md` §2, requisito R2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Latencia {
    /// Mediana, em milissegundos.
    pub mediana_ms: u32,
    /// Percentil 99, em milissegundos.
    pub p99_ms: u32,
    /// Quantas amostras entraram na conta.
    pub amostras: u32,
}

impl Latencia {
    /// Se a latência está dentro da meta do portador dado.
    ///
    /// Metas de `docs/01-visao-e-escopo.md` §6. Serve para a interface poder dizer "está boa"
    /// em vez de só mostrar um número que o usuário não sabe interpretar.
    #[must_use]
    pub const fn dentro_da_meta(&self, portador: Portador) -> bool {
        let (mediana, p99) = match portador {
            Portador::Bluetooth => (20, 50),
            Portador::RedeLocal | Portador::RedeDeArquivos => (8, 25),
        };
        self.mediana_ms <= mediana && self.p99_ms <= p99
    }
}

/// O par, resumido para a interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParConhecido {
    /// Identificador da instalação.
    pub maquina: Maquina,
    /// Nome legível.
    pub nome: Nome,
    /// O que ele declarou saber fazer.
    pub recursos: Recursos,
    /// Se está conectado agora.
    pub conectado: bool,
}

/// Por que a última sessão terminou.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MotivoDaQueda {
    /// O usuário mandou parar.
    PedidoPeloUsuario,
    /// O serviço do outro lado está parando.
    ServicoDoParParando,
    /// O outro computador foi suspenso.
    ParSuspenso,
    /// O outro computador parou de responder.
    ParNaoRespondeu,
    /// O meio de conexão falhou: rádio desligado, cabo removido, socket fechado.
    MeioFalhou,
    /// Erro de protocolo.
    ErroDeProtocolo,
    /// Troca de meio em andamento.
    TrocandoDeMeio,
}

impl MotivoDaQueda {
    /// A frase que a interface mostra.
    ///
    /// Cada uma diz **o que aconteceu**, não "erro". No v1, toda queda parecia igual, e
    /// diagnosticar era impossível ([00, §6](../../../docs/00-licoes-do-v1.md)).
    #[must_use]
    pub const fn frase(self) -> &'static str {
        match self {
            Self::PedidoPeloUsuario => "Você encerrou a conexão",
            Self::ServicoDoParParando => "O serviço do outro computador está parando",
            Self::ParSuspenso => "O outro computador foi suspenso",
            Self::ParNaoRespondeu => "O outro computador parou de responder",
            Self::MeioFalhou => "O meio de conexão falhou",
            Self::ErroDeProtocolo => "Erro de protocolo entre as duas versões",
            Self::TrocandoDeMeio => "Trocando de meio de conexão",
        }
    }
}

/// Tudo que a interface precisa saber, num só valor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Estado {
    /// Em que ponto a sessão está.
    pub enlace: LinkState,
    /// O papel desta máquina.
    pub papel: Papel,
    /// De que lado fica a outra tela.
    pub borda_do_par: Borda,
    /// Esta máquina, para a impressão digital aparecer na tela de pareamento.
    pub esta_maquina: Maquina,
    /// O nome desta máquina.
    pub este_nome: Nome,
    /// O par pareado, se houver.
    pub par: Option<ParConhecido>,
    /// O portador em uso.
    pub portador: Option<Portador>,
    /// O portador fixado nas preferências, se algum.
    ///
    /// Distinto de [`Self::portador`]: aquele é o que está valendo agora, este é a preferência
    /// gravada. A interface precisa dos dois — mostra um e permite editar o outro.
    pub portador_fixado: Option<Portador>,
    /// Por que ele foi escolhido.
    pub motivo_do_portador: Option<MotivoDoPortador>,
    /// A latência medida.
    pub latencia: Option<Latencia>,
    /// Até onde esta máquina consegue injetar sem sessão desbloqueada.
    pub nivel_privilegiado: Nivel,
    /// Se o agente local está pronto.
    pub agente_pronto: bool,
    /// Se a digitação na tela de bloqueio está permitida para o par.
    ///
    /// É uma permissão gravada, e não uma capacidade: [`Self::nivel_privilegiado`] diz se a
    /// máquina *consegue*, este campo diz se o usuário *deixou*. Os dois precisam ser
    /// verdadeiros, e separá-los é o que permite a interface dizer qual dos dois falta.
    pub bloqueio_permitido: bool,
    /// Por que a última sessão terminou.
    pub ultima_queda: Option<MotivoDaQueda>,
}

impl Estado {
    /// Um estado de máquina recém-instalada: nada pareado, nada conectado.
    #[must_use]
    pub fn recem_instalado(maquina: Maquina, nome: Nome) -> Self {
        Self {
            enlace: LinkState::Desconectado,
            papel: Papel::Servidor,
            borda_do_par: Borda::Direita,
            esta_maquina: maquina,
            este_nome: nome,
            par: None,
            portador: None,
            portador_fixado: None,
            motivo_do_portador: None,
            latencia: None,
            nivel_privilegiado: Nivel::Nenhum,
            agente_pronto: false,
            bloqueio_permitido: false,
            ultima_queda: None,
        }
    }

    /// A frase principal que a interface mostra, em uma linha.
    ///
    /// É a coisa mais importante da tela: se ela responder "por que não está funcionando?"
    /// sozinha, o usuário não precisa procurar em mais lugar nenhum.
    #[must_use]
    pub fn resumo(&self) -> String {
        match (self.enlace, self.par.as_ref()) {
            (LinkState::Desconectado, None) => {
                "Nenhum computador pareado. Pareie um para começar.".to_owned()
            }
            (LinkState::Desconectado, Some(par)) => match self.ultima_queda {
                Some(motivo) => format!("{} — {}", par.nome, motivo.frase()),
                None => format!("{} está pareado, mas não está por perto.", par.nome),
            },
            (LinkState::Conectando, Some(par)) => format!("Conectando a {}…", par.nome),
            (LinkState::Conectando, None) => "Conectando…".to_owned(),
            (LinkState::Pronto, Some(par)) => {
                format!("Conectado a {}. Leve o ponteiro até a borda.", par.nome)
            }
            (LinkState::EmUso, Some(par)) => format!("Controlando {}.", par.nome),
            (_, None) => self.enlace.frase().to_owned(),
        }
    }

    /// O que impede o produto de funcionar agora, se algo impedir.
    ///
    /// Devolve a frase de um único problema — o mais grave. Mostrar cinco avisos ao mesmo tempo
    /// é a mesma coisa que não mostrar nenhum.
    #[must_use]
    pub fn impedimento(&self) -> Option<&'static str> {
        if !self.agente_pronto {
            return Some(
                "O componente que digita nesta máquina não está pronto. \
                 Teclado e mouse remotos não vão funcionar.",
            );
        }
        if self.papel == Papel::Cliente && !self.nivel_privilegiado.suficiente() {
            return Some(
                "Esta máquina não aceita digitação na tela de bloqueio. \
                 Ver as instruções em Preferências.",
            );
        }
        // Não conseguir e não ter deixado são problemas diferentes, com soluções também
        // diferentes. Dizer "não aceita" a quem só precisa ligar uma opção manda a pessoa
        // investigar instalação e assinatura para nada.
        if self.papel == Papel::Cliente && !self.bloqueio_permitido {
            return Some(
                "A digitação na tela de bloqueio está desligada. Ligue em Preferências \
                 para poder desbloquear esta máquina do outro computador.",
            );
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cliente_pronto() -> Estado {
        let mut estado = Estado::recem_instalado(Maquina([3; 16]), Nome::coagido("cliente"));
        estado.papel = Papel::Cliente;
        estado.agente_pronto = true;
        estado.nivel_privilegiado = Nivel::TelaDeBloqueio;
        estado.bloqueio_permitido = true;
        estado
    }

    #[test]
    fn o_agente_ausente_e_o_impedimento_mais_grave() {
        // Sem o agente nada funciona, então ele precisa vencer qualquer outro aviso — mostrar
        // "ligue a opção de tela de bloqueio" quando o componente que digita nem subiu manda o
        // usuário resolver o problema errado.
        let mut estado = cliente_pronto();
        estado.agente_pronto = false;
        estado.nivel_privilegiado = Nivel::Nenhum;
        estado.bloqueio_permitido = false;
        let frase = estado.impedimento().expect("há impedimento");
        assert!(frase.contains("digita nesta máquina"), "{frase}");
    }

    #[test]
    fn nao_conseguir_e_nao_ter_deixado_sao_impedimentos_distintos() {
        let mut sem_capacidade = cliente_pronto();
        sem_capacidade.nivel_privilegiado = Nivel::SoDesbloqueado;

        let mut sem_permissao = cliente_pronto();
        sem_permissao.bloqueio_permitido = false;

        assert_ne!(
            sem_capacidade.impedimento(),
            sem_permissao.impedimento(),
            "as duas causas pedem ações diferentes do usuário"
        );
        assert!(
            sem_permissao
                .impedimento()
                .expect("há impedimento")
                .contains("Preferências")
        );
    }

    #[test]
    fn um_cliente_capaz_e_permitido_nao_tem_impedimento() {
        assert_eq!(cliente_pronto().impedimento(), None);
    }

    #[test]
    fn o_servidor_nao_e_cobrado_pela_tela_de_bloqueio_do_par() {
        // Quem tem o teclado não precisa aceitar digitação na própria tela de bloqueio para o
        // produto cumprir o requisito: é o lado controlado que precisa.
        let mut estado = cliente_pronto();
        estado.papel = Papel::Servidor;
        estado.nivel_privilegiado = Nivel::Nenhum;
        estado.bloqueio_permitido = false;
        assert_eq!(estado.impedimento(), None);
    }

    #[test]
    fn o_portador_em_uso_e_o_fixado_sao_campos_distintos() {
        // Um usuário que fixou Bluetooth e está na rede precisa ver as duas coisas: senão a
        // interface mostra "rede" e a preferência dele parece ter sido ignorada sem explicação.
        let mut estado = cliente_pronto();
        estado.portador_fixado = Some(Portador::Bluetooth);
        estado.portador = Some(Portador::RedeLocal);
        assert_ne!(estado.portador, estado.portador_fixado);
    }

    #[test]
    fn a_meta_de_latencia_e_mais_folgada_no_bluetooth() {
        let medida = Latencia {
            mediana_ms: 15,
            p99_ms: 40,
            amostras: 500,
        };
        assert!(medida.dentro_da_meta(Portador::Bluetooth));
        assert!(
            !medida.dentro_da_meta(Portador::RedeLocal),
            "na rede 15 ms de mediana já é ruim"
        );
    }
}
