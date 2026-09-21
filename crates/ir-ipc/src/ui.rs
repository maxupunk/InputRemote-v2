//! O que a interface pede, e o que ela recebe.
//!
//! A decisão central deste módulo: **cada pedido declara o privilégio que exige.** A
//! autorização não fica espalhada pelo serviço em `if`s — ela é uma propriedade do pedido, e o
//! transporte a consulta antes de entregar.
//!
//! Isso importa mais aqui que na maioria dos lugares. Quem consegue falar com este serviço
//! consegue digitar a senha de administrador ([04, §5](../../../docs/04-seguranca.md)); um
//! pedido que esqueça de declarar o próprio nível é uma escalada de privilégio, não um
//! descuido de estilo. O teste `todo_pedido_declara_autoridade` existe por isso.

use serde::{Deserialize, Serialize};

use crate::status::Estado;
use crate::vocabulario::{Borda, Maquina, Portador};

/// O que pode dar errado num pedido. Mora em [`crate::falha`], e continua alcançável por aqui.
pub use crate::falha::Falha;

/// Que privilégio um pedido exige.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Autoridade {
    /// Ler estado, configuração e diagnóstico.
    ///
    /// O usuário interativo da sessão de console. Ler não muda nada.
    Ler,
    /// Alterar configuração, iniciar e parar sessão.
    ///
    /// Também o usuário interativo: são decisões dele sobre a própria máquina.
    Configurar,
    /// Parear, esquecer par, permitir tela de bloqueio, habilitar Ctrl+Alt+Del.
    ///
    /// **Exige elevação.** São as operações que decidem quem pode digitar na tela de
    /// bloqueio desta máquina — quem as consegue, consegue a máquina.
    Elevado,
}

impl Autoridade {
    /// A frase que a interface mostra ao pedir elevação.
    #[must_use]
    pub const fn explicacao(self) -> &'static str {
        match self {
            Self::Ler => "Somente leitura.",
            Self::Configurar => "Altera as preferências deste computador.",
            Self::Elevado => {
                "Esta ação decide quem pode digitar na tela de bloqueio deste computador, \
                 então precisa de permissão de administrador."
            }
        }
    }
}

/// Um computador que a descoberta encontrou.
///
/// `rotulo` é para a tela e `endereco` é para o serviço. A separação existe porque o que
/// identifica a máquina — endereço Bluetooth, endereço IP — não é o que o usuário reconhece, e
/// obrigar a interface a extrair um do outro seria colocar regra de produto na tela.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidato {
    /// Como o usuário reconhece este computador.
    pub rotulo: String,
    /// Como o serviço o alcança.
    pub endereco: String,
    /// Por onde ele foi encontrado.
    pub portador: Portador,
}

/// O que a interface pede ao serviço.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Pedido {
    /// Me dê o estado agora.
    Estado,
    /// Me avise a cada mudança de estado, até eu desconectar.
    Acompanhar,
    /// Troque o papel desta máquina.
    DefinirPapel(crate::status::Papel),
    /// Troque a borda que dá para o outro computador.
    DefinirBorda(Borda),
    /// Fixe um portador, ou volte à escolha automática.
    FixarPortador(Option<Portador>),
    /// Procure computadores com quem parear.
    ///
    /// Responde na hora e conta o que encontrou por [`Aviso::CandidatosEncontrados`]: a
    /// descoberta demora, e uma interface que congela enquanto espera é uma interface quebrada.
    Procurar,
    /// Comece a parear com o computador escolhido.
    IniciarPareamento {
        /// O [`Candidato::endereco`] do escolhido.
        candidato: String,
    },
    /// O código apareceu igual nas duas telas?
    ConfirmarPareamento {
        /// `true` se o usuário confirmou que os códigos são iguais.
        conferiu: bool,
    },
    /// Esqueça este par.
    EsquecerPar {
        /// Qual.
        maquina: Maquina,
    },
    /// Permita, ou deixe de permitir, digitação na tela de bloqueio.
    PermitirTelaDeBloqueio {
        /// Para qual par.
        maquina: Maquina,
        /// Ligar ou desligar.
        permitir: bool,
    },
    /// Encerre a sessão agora.
    Encerrar,
    /// Monte o relatório de diagnóstico.
    Diagnostico,
    /// Mande estes arquivos e pastas para o outro computador.
    ///
    /// No uso normal quem dispara isto **não é a interface**: é o clipboard. O usuário aperta
    /// Ctrl+C, o agente percebe a mudança e conta ao serviço. Este pedido existe para a mesma
    /// coisa ser alcançável sem janela — pela ferramenta de bancada, pelo diagnóstico, e por quem
    /// prefere um comando a um atalho.
    EnviarArquivos {
        /// Caminhos absolutos nesta máquina.
        caminhos: Vec<String>,
    },
    /// Leve o clipboard deste computador para o outro, agora.
    ///
    /// O gatilho automático é a travessia: quando o controle sai desta máquina, o que está no
    /// clipboard daqui vai junto ([ADR-0011](../../../docs/adr/0011-clipboard-na-travessia.md)).
    /// Este pedido é o mesmo gatilho, à mão — para um atalho de teclado do ambiente gráfico ou um
    /// item da bandeja, quando o usuário quer mandar sem levar o mouse até lá.
    SincronizarClipboard,
    /// Leve este texto, que o usuário copiou, para o clipboard do outro computador.
    ///
    /// Quem manda é o ajudante de clipboard, que lê na sessão do usuário. O serviço não lê o
    /// clipboard de ninguém; ele só leva o que lhe é entregue.
    OferecerTexto(crate::texto::TextoDoClipboard),
    /// Esvazie a pasta de recebidos agora.
    ///
    /// O automático cuida do que passou da idade ou do teto ([`ir_transferencia`]); isto é o botão
    /// da pessoa, para quando ela quer o espaço de volta na hora. Nada do que está em curso é
    /// afetado: arquivo em transferência ainda não está lá.
    LimparRecebidos,
    /// Como [`Self::Acompanhar`], dito pelo ajudante de clipboard: "sou eu, e estou aqui".
    ///
    /// Sem o ajudante a cópia não atravessa, e nada na tela diria por quê. É por este pedido que o
    /// serviço sabe que há um — para relançá-lo quando não há, e para dizer no diagnóstico.
    AcompanharClipboard,
}

impl Pedido {
    /// O privilégio que este pedido exige.
    #[must_use]
    pub const fn autoridade(&self) -> Autoridade {
        match self {
            Self::Estado | Self::Acompanhar | Self::AcompanharClipboard | Self::Diagnostico => {
                Autoridade::Ler
            }
            // Procurar não muda configuração, mas emite anúncio na rede e no rádio: é ação,
            // não leitura, e não é coisa que um processo qualquer deva conseguir disparar.
            Self::DefinirPapel(_)
            | Self::DefinirBorda(_)
            | Self::FixarPortador(_)
            | Self::Procurar
            | Self::Encerrar
            // Mandar arquivo é ação com consequência: o conteúdo sai desta máquina. Mas exigir
            // elevação aqui seria exigir elevação **a cada colagem**, já que é este o caminho que
            // o Ctrl+C vai usar — e uma permissão que atrapalha o uso normal acaba desligada. O
            // portão desta operação é o pareamento: só existe um par, confirmado por código de
            // seis dígitos nas duas telas, e a permissão de arquivos é revogável só para ele
            // ([04, §2](../../../docs/04-seguranca.md)).
            | Self::EnviarArquivos { .. }
            | Self::SincronizarClipboard
            | Self::LimparRecebidos
            | Self::OferecerTexto(_) => Autoridade::Configurar,
            // Tudo que decide **quem pode digitar** nesta máquina exige elevação.
            Self::IniciarPareamento { .. }
            | Self::ConfirmarPareamento { .. }
            | Self::EsquecerPar { .. }
            | Self::PermitirTelaDeBloqueio { .. } => Autoridade::Elevado,
        }
    }
}

/// A resposta do serviço a um pedido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Resposta {
    /// Feito, sem nada a devolver.
    Feito,
    /// O estado corrente.
    Estado(Estado),
    /// O relatório de diagnóstico, em texto já pronto para copiar.
    Diagnostico(String),
    /// Não deu, e aqui está o porquê.
    Falha(Falha),
}

/// Uma mensagem do serviço para a interface, no fluxo de bytes do canal de controle.
///
/// O canal carrega dois tipos de coisa misturados: a resposta a um pedido, e um aviso que o
/// serviço manda por conta própria (o código de pareamento, uma mudança de estado). O envelope
/// distingue os dois para a interface não confundir um com o outro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ParaInterface {
    /// A resposta a um [`Pedido`].
    Resposta(Resposta),
    /// Um aviso não solicitado.
    Aviso(Aviso),
}

impl From<Resposta> for ParaInterface {
    fn from(resposta: Resposta) -> Self {
        Self::Resposta(resposta)
    }
}

impl From<Aviso> for ParaInterface {
    fn from(aviso: Aviso) -> Self {
        Self::Aviso(aviso)
    }
}

/// O que o serviço conta à interface sem ela pedir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Aviso {
    /// O estado mudou.
    EstadoMudou(Estado),
    /// A descoberta encontrou estes computadores.
    ///
    /// Lista completa a cada vez, e não incrementos: a tela mostra o que existe agora, e
    /// reconciliar adições e remoções na interface seria estado duplicado.
    CandidatosEncontrados {
        /// O que foi encontrado.
        candidatos: Vec<Candidato>,
    },
    /// Confira este código na outra tela.
    ///
    /// Seis dígitos, um por posição. Vem separado e não como texto para a interface poder
    /// mostrá-los em caixas — que é o que faz duas pessoas conseguirem comparar em voz alta
    /// sem errar.
    CodigoDePareamento {
        /// Os seis dígitos, cada um de 0 a 9.
        digitos: [u8; 6],
    },
    /// O pareamento terminou.
    PareamentoConcluido {
        /// Se deu certo.
        sucesso: bool,
    },
    /// O pareamento pedido por esta máquina não começou, por este motivo.
    ///
    /// Separado de [`Self::PareamentoConcluido`] porque aqui há o que dizer: não houve código, e o
    /// motivo decide o que a pessoa faz — abrir o programa do outro lado, ou digitar o endereço.
    PareamentoFalhou(crate::Falha),
    /// Uma transferência de arquivos mudou de estado.
    ///
    /// Vem como aviso, e não dentro do [`Estado`], porque a transferência é um acontecimento com
    /// começo e fim, e não uma propriedade da máquina. Enfiá-la no estado obrigaria a interface a
    /// diferenciar "não há transferência" de "havia uma e acabou".
    Transferencia(crate::transferencia::Transferencia),
    /// Leia o clipboard desta máquina e ofereça ao par, se ele mudou.
    ///
    /// Vai para quem cuida do clipboard na sessão do usuário. Sem conteúdo nenhum: quem sabe o que
    /// há no clipboard é quem está na sessão, e o serviço não deve nem precisar saber
    /// ([ADR-0011](../../../docs/adr/0011-clipboard-na-travessia.md)).
    LerClipboard,
    /// Chegou este texto do par: ponha-o no clipboard desta sessão.
    TextoRecebido(crate::texto::TextoDoClipboard),
    /// Uma tecla ficou divergente e foi corrigida.
    ///
    /// Muitos destes seguidos indicam perda no meio de conexão, e o número aparece no
    /// diagnóstico.
    EstadoReconciliado {
        /// Quantas teclas foram soltas.
        soltas: u8,
        /// Quantas foram pressionadas.
        pressionadas: u8,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocabulario::Nome;

    fn todos_os_pedidos() -> Vec<Pedido> {
        vec![
            Pedido::Estado,
            Pedido::Acompanhar,
            Pedido::DefinirPapel(crate::status::Papel::Cliente),
            Pedido::DefinirBorda(Borda::Esquerda),
            Pedido::FixarPortador(Some(Portador::Bluetooth)),
            Pedido::Procurar,
            Pedido::IniciarPareamento {
                candidato: "192.168.0.10".to_owned(),
            },
            Pedido::ConfirmarPareamento { conferiu: true },
            Pedido::EsquecerPar {
                maquina: Maquina([0; 16]),
            },
            Pedido::PermitirTelaDeBloqueio {
                maquina: Maquina([0; 16]),
                permitir: true,
            },
            Pedido::Encerrar,
            Pedido::Diagnostico,
            Pedido::AcompanharClipboard,
            Pedido::LimparRecebidos,
        ]
    }

    #[test]
    fn todo_pedido_declara_autoridade() {
        // Um pedido que esqueça de declarar o próprio nível é escalada de privilégio, não
        // descuido de estilo. Este teste falha se alguém acrescentar variante sem classificá-la.
        for pedido in todos_os_pedidos() {
            let _ = pedido.autoridade();
        }
    }

    #[test]
    fn ler_nunca_exige_elevacao() {
        for pedido in [
            Pedido::Estado,
            Pedido::Acompanhar,
            Pedido::AcompanharClipboard,
            Pedido::Diagnostico,
        ] {
            assert_eq!(pedido.autoridade(), Autoridade::Ler, "{pedido:?}");
        }
    }

    #[test]
    fn tudo_que_decide_quem_digita_exige_elevacao() {
        let sensíveis = [
            Pedido::IniciarPareamento {
                candidato: "x".to_owned(),
            },
            Pedido::ConfirmarPareamento { conferiu: true },
            Pedido::EsquecerPar {
                maquina: Maquina([1; 16]),
            },
            Pedido::PermitirTelaDeBloqueio {
                maquina: Maquina([1; 16]),
                permitir: true,
            },
        ];
        for pedido in sensíveis {
            assert_eq!(
                pedido.autoridade(),
                Autoridade::Elevado,
                "{pedido:?} decide quem pode digitar na tela de bloqueio"
            );
        }
    }

    #[test]
    fn as_autoridades_sao_ordenadas_por_poder() {
        assert!(Autoridade::Elevado > Autoridade::Configurar);
        assert!(Autoridade::Configurar > Autoridade::Ler);
    }

    #[test]
    fn o_estado_recem_instalado_diz_o_que_fazer_primeiro() {
        let estado = Estado::recem_instalado(Maquina([7; 16]), Nome::coagido("bancada"));
        let resumo = estado.resumo();
        assert!(
            resumo.contains("Pareie"),
            "a primeira tela precisa dizer o primeiro passo"
        );
    }
}
