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
    /// Pronto: cada um usa a própria tela.
    Pronto,
    /// O teclado e o mouse daqui estão controlando o outro computador.
    Controlando,
    /// O outro computador está usando esta tela.
    Controlado,
}

impl LinkState {
    /// A frase que a interface mostra.
    #[must_use]
    pub const fn frase(self) -> &'static str {
        match self {
            Self::Desconectado => "Desconectado",
            Self::Conectando => "Conectando…",
            Self::Pronto => "Pronto",
            Self::Controlando => "Controlando o outro computador",
            Self::Controlado => "Controlado pelo outro computador",
        }
    }

    /// Se há sessão de pé.
    #[must_use]
    pub const fn conectado(self) -> bool {
        matches!(self, Self::Pronto | Self::Controlando | Self::Controlado)
    }
}

/// Quem pode controlar quem ([ADR-0014](../../../docs/adr/0014-controle-simetrico.md)).
///
/// Não é um papel: com [`Politica::Ambos`], o padrão, qualquer um dos dois computadores leva o
/// controle ao outro, e quem está usando agora é só o [`LinkState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Politica {
    /// Os dois controlam um ao outro.
    #[default]
    Ambos,
    /// Este controla o outro, e nunca é controlado.
    SoEste,
    /// Este é controlado pelo outro, e nunca o controla.
    SoOOutro,
}

impl Politica {
    /// A explicação curta de cada opção, para a tela.
    #[must_use]
    pub const fn frase(self) -> &'static str {
        match self {
            Self::Ambos => {
                "O teclado e o mouse de cada computador controlam o outro: leve o ponteiro até a \
                 borda para ir, e mexa no mouse daqui para voltar."
            }
            Self::SoEste => {
                "O teclado e o mouse daqui controlam o outro computador, e o outro nunca vem para cá."
            }
            Self::SoOOutro => {
                "O teclado e o mouse do outro computador controlam este, e os daqui ficam só aqui."
            }
        }
    }

    /// Se o teclado e o mouse daqui podem ir para o outro computador.
    #[must_use]
    pub const fn manda(self) -> bool {
        !matches!(self, Self::SoOOutro)
    }

    /// Se o outro computador pode vir para cá.
    #[must_use]
    pub const fn recebe(self) -> bool {
        !matches!(self, Self::SoEste)
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
    /// Bluetooth e rede local juntos: cada comando vai pelos dois, e vale o que chegar primeiro.
    ///
    /// No fim do enum, e não junto dos outros: o `postcard` grava a variante pelo índice.
    Redundancia,
}

impl MotivoDoPortador {
    /// A frase que a interface mostra.
    #[must_use]
    pub const fn frase(self) -> &'static str {
        match self {
            Self::Preferido => "Bluetooth disponível, que dá a latência mais constante",
            Self::RedeComoAlternativa => "Bluetooth indisponível; usando a rede local",
            Self::FixadoPeloUsuario => "Fixado nas preferências",
            Self::Redundancia => {
                "Bluetooth e rede local juntos: cada comando vai pelos dois e vale o que chegar primeiro"
            }
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

/// Tudo que a interface precisa saber, num só valor.
///
/// Os campos lógicos são fatos independentes que a tela mostra lado a lado — conectado, pausado,
/// borda travada —, e não um estado só disfarçado de vários.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Estado {
    /// Em que ponto a sessão está.
    pub enlace: LinkState,
    /// Quem pode controlar quem.
    pub politica: Politica,
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
    /// Quanto a pasta de recebidos ocupa agora, em bytes.
    ///
    /// O que chega precisa existir em algum lugar para ser colado, e depois sobra. O serviço tira
    /// sozinho o que passou da idade ou do teto; este número existe para a pessoa ver o que ainda
    /// está lá — e poder esvaziar quando quiser, que é a parte que o produto não decide por ela.
    pub recebidos_bytes: u64,
    /// Se a sessão fala pelo Bluetooth **e** pela rede ao mesmo tempo.
    ///
    /// Aí [`Self::portador`] é o Bluetooth, o preferido, e a tela mostra os dois
    /// ([`Self::nome_da_rota`]). No fim da estrutura porque o `postcard` é posicional.
    pub rota_dupla: bool,
    /// A economia de energia do Wi-Fi desta máquina, quando atrapalha.
    pub economia_aqui: Option<EconomiaDoWifi>,
    /// A economia de energia do Wi-Fi do par, quando atrapalha e ele contou.
    pub economia_no_par: Option<EconomiaDoWifi>,
    /// Se o compartilhamento está pausado, e de que lado.
    pub pausa: Option<Pausa>,
    /// Se o outro computador está numa tela protegida — bloqueio, login, UAC — e descartando o
    /// que se digita daqui, por não ter dado a permissão.
    pub par_recusa_tela_de_bloqueio: bool,
    /// Onde os arquivos recebidos ficam, para o botão "Abrir a pasta".
    pub pasta_de_recebidos: String,
    /// Se a borda está travada: o ponteiro não atravessa, só o atalho leva o controle.
    pub borda_travada: bool,
    /// Se o outro computador bloqueia junto quando este bloquear.
    pub bloquear_juntos: bool,
    /// Se esta máquina consegue ler o próprio teclado e mouse, para controlar o outro.
    ///
    /// Distinto de [`Self::agente_pronto`], que é conseguir **receber**: no Linux a captura e a
    /// injeção são peças separadas, e uma pode faltar sem a outra.
    pub captura_pronta: bool,
}

impl Estado {
    /// Por onde a sessão fala, como a tela mostra: um portador, ou os dois.
    #[must_use]
    pub fn nome_da_rota(&self) -> &'static str {
        match self.portador {
            Some(_) if self.rota_dupla => "Bluetooth + Rede local",
            Some(portador) => portador.nome(),
            None => "",
        }
    }

    /// A frase curta do enlace, do ponto de vista desta máquina.
    #[must_use]
    pub const fn frase_do_enlace(&self) -> &'static str {
        self.enlace.frase()
    }

    /// Um estado de máquina recém-instalada: nada pareado, nada conectado.
    #[must_use]
    pub fn recem_instalado(maquina: Maquina, nome: Nome) -> Self {
        Self {
            enlace: LinkState::Desconectado,
            politica: Politica::Ambos,
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
            recebidos_bytes: 0,
            rota_dupla: false,
            economia_aqui: None,
            economia_no_par: None,
            pausa: None,
            par_recusa_tela_de_bloqueio: false,
            pasta_de_recebidos: String::new(),
            borda_travada: false,
            bloquear_juntos: true,
            captura_pronta: false,
        }
    }
}

mod avisos;
mod frases;
mod queda;

pub use avisos::{AvisoDeRede, EconomiaDoWifi, Pausa};
pub use queda::MotivoDaQueda;

#[cfg(test)]
mod testes;
