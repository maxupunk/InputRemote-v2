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

/// O que o serviço conta sem a interface pedir. Mora em [`crate::aviso`], e continua alcançável por
/// aqui.
pub use crate::aviso::Aviso;
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
    /// Mude quem pode controlar quem.
    DefinirPolitica(crate::status::Politica),
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
    /// Pause o compartilhamento: a sessão cai, o par é avisado, e ninguém disca até
    /// [`Pedido::Retomar`].
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
    /// Desligue a economia de energia do Wi-Fi — desta máquina, ou do par.
    ///
    /// É o botão do aviso de rede: com a economia ligada a placa cochila entre pacotes, e o mouse
    /// pela rede trava em rajadas. No fim do enum, porque o `postcard` grava a variante pelo índice.
    DesligarEconomiaDeEnergia {
        /// `true` para pedir ao outro computador, pela sessão; `false` para esta máquina.
        no_par: bool,
    },
    /// Volte a compartilhar depois de uma pausa ([`Pedido::Encerrar`] pausa).
    ///
    /// Encerrar só derrubava os enlaces, e a reconexão discava de novo em 3 s: o botão parecia não
    /// funcionar. Agora encerrar é pausar de verdade — o par é avisado e ninguém disca — até isto.
    Retomar,
    /// Pare a cópia de arquivos em curso. O que já chegou do outro lado é descartado inteiro.
    CancelarCopia,
    /// Mande Ctrl+Alt+Del ao outro computador — o botão, além do atalho Ctrl+Alt+End.
    CtrlAltDel,
    /// Trave, ou destrave, a borda: travada, o ponteiro não atravessa — só o atalho leva o controle.
    TravarBorda(bool),
    /// Bloquear, ou não, o outro computador quando este bloquear.
    BloquearJuntos(bool),
    /// As pastas compartilhadas, como estão agora ([`Resposta::Pastas`]).
    Pastas,
    /// Um pedido sobre as pastas compartilhadas, que o serviço repassa ao ajudante das pastas.
    Pasta(crate::pastas::ComandoDePasta),
    /// Ligue, ou desligue, copiar e colar entre os dois computadores.
    CopiarEColar(bool),
    /// Os painéis no Android pareados, como estão agora ([`Resposta::Paineis`]).
    Paineis,
    /// Um pedido sobre os painéis no Android.
    Painel(crate::paineis::ComandoDePainel),
    /// Religue o rádio Bluetooth deste computador: o botão do aviso de Bluetooth parado
    /// ([`crate::BluetoothParado`]). No fim do enum, porque o `postcard` grava a variante pelo
    /// índice.
    ReligarBluetooth,
}

impl Pedido {
    /// O privilégio que este pedido exige.
    #[must_use]
    pub const fn autoridade(&self) -> Autoridade {
        match self {
            Self::Estado
            | Self::Acompanhar
            | Self::AcompanharClipboard
            | Self::Diagnostico
            | Self::Pastas
            | Self::Paineis => Autoridade::Ler,
            // Procurar não muda configuração, mas emite anúncio na rede e no rádio: é ação,
            // não leitura, e não é coisa que um processo qualquer deva conseguir disparar.
            Self::DefinirPolitica(_)
            | Self::DefinirBorda(_)
            | Self::FixarPortador(_)
            | Self::Procurar
            | Self::Encerrar
            | Self::Retomar
            | Self::CancelarCopia
            | Self::CtrlAltDel
            | Self::TravarBorda(_)
            | Self::BloquearJuntos(_)
            | Self::CopiarEColar(_)
            // O painel só lê: carga, temperatura e latência, nada de entrada nem de conteúdo. Parear
            // um é decisão da pessoa sobre a própria máquina, e não sobre quem digita nela.
            | Self::Painel(_)
            // Compartilhar uma pasta põe o conteúdo dela no outro computador: o mesmo portão de
            // mandar arquivo — o pareamento —, e pelo mesmo motivo.
            | Self::Pasta(_)
            // Mandar arquivo é ação com consequência: o conteúdo sai desta máquina. Mas exigir
            // elevação aqui seria exigir elevação **a cada colagem**, já que é este o caminho que
            // o Ctrl+C vai usar — e uma permissão que atrapalha o uso normal acaba desligada. O
            // portão desta operação é o pareamento: só existe um par, confirmado por código de
            // seis dígitos nas duas telas, e a permissão de arquivos é revogável só para ele
            // ([04, §2](../../../docs/04-seguranca.md)).
            | Self::EnviarArquivos { .. }
            | Self::SincronizarClipboard
            | Self::LimparRecebidos
            | Self::OferecerTexto(_)
            // Muda a configuração de energia da máquina, e mais nada: o mesmo nível de esvaziar
            // os recebidos, e não o de decidir quem digita. Religar o rádio, idem.
            | Self::DesligarEconomiaDeEnergia { .. }
            | Self::ReligarBluetooth => Autoridade::Configurar,
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
    /// As pastas compartilhadas.
    Pastas(Vec<crate::pastas::ResumoDePasta>),
    /// Os painéis no Android pareados.
    Paineis(Vec<crate::paineis::ResumoDoPainel>),
}

impl Resposta {
    /// [`Self::Feito`] se deu certo; senão, a falha que diz por quê. A forma de quase toda ação do
    /// serviço, escrita uma vez.
    #[must_use]
    pub const fn feito_ou(deu_certo: bool, falha: Falha) -> Self {
        if deu_certo {
            Self::Feito
        } else {
            Self::Falha(falha)
        }
    }
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

#[cfg(test)]
mod tests;
