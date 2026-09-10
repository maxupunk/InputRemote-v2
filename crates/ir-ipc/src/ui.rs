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
}

impl Pedido {
    /// O privilégio que este pedido exige.
    #[must_use]
    pub const fn autoridade(&self) -> Autoridade {
        match self {
            Self::Estado | Self::Acompanhar | Self::Diagnostico => Autoridade::Ler,
            // Procurar não muda configuração, mas emite anúncio na rede e no rádio: é ação,
            // não leitura, e não é coisa que um processo qualquer deva conseguir disparar.
            Self::DefinirPapel(_)
            | Self::DefinirBorda(_)
            | Self::FixarPortador(_)
            | Self::Procurar
            | Self::Encerrar => Autoridade::Configurar,
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

/// O que pode dar errado num pedido.
///
/// Toda variante tem uma frase que diz **o que fazer agora** — a terceira parte que
/// [09, §5](../../../docs/09-padroes-de-codigo.md) exige e que a maioria dos produtos esquece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[non_exhaustive]
pub enum Falha {
    /// Falta privilégio.
    #[error("esta ação precisa de permissão de administrador")]
    PrecisaElevacao,
    /// O pedido não faz sentido no estado atual.
    #[error("esta ação não faz sentido agora")]
    ForaDeContexto,
    /// O par indicado não existe.
    #[error("esse computador não está pareado")]
    ParDesconhecido,
    /// O pareamento expirou sem confirmação.
    #[error("o código expirou")]
    PareamentoExpirou,
    /// O usuário disse que os códigos não conferem.
    #[error("os códigos não conferiam")]
    CodigosDiferentes,
    /// Falha interna do serviço.
    #[error("falha interna do serviço")]
    Interna,
}

impl Falha {
    /// O que o usuário deve fazer agora.
    #[must_use]
    pub const fn o_que_fazer(self) -> &'static str {
        match self {
            Self::PrecisaElevacao => {
                "Feche e abra o InputRemote como administrador para concluir esta ação."
            }
            Self::ForaDeContexto => "Confira o estado da conexão e tente de novo.",
            Self::ParDesconhecido => "Pareie o computador antes de configurá-lo.",
            Self::PareamentoExpirou => "Comece o pareamento de novo; o código vale 2 minutos.",
            Self::CodigosDiferentes => {
                "Códigos diferentes significam que alguém pode estar no meio da conexão. \
                 Não pareie por esta rede e procure ajuda."
            }
            Self::Interna => {
                "Exporte o diagnóstico em Preferências e abra um relato com ele anexado."
            }
        }
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
        for pedido in [Pedido::Estado, Pedido::Acompanhar, Pedido::Diagnostico] {
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
    fn toda_falha_diz_o_que_fazer() {
        let falhas = [
            Falha::PrecisaElevacao,
            Falha::ForaDeContexto,
            Falha::ParDesconhecido,
            Falha::PareamentoExpirou,
            Falha::CodigosDiferentes,
            Falha::Interna,
        ];
        for falha in falhas {
            assert!(!falha.to_string().is_empty(), "{falha:?} sem descrição");
            let acao = falha.o_que_fazer();
            assert!(!acao.is_empty(), "{falha:?} não diz o que fazer");
            // Uma instrução tem verbo. É o mínimo para ser acionável.
            assert!(
                acao.len() > 20,
                "{falha:?}: `{acao}` é curto demais para instruir"
            );
        }
    }

    #[test]
    fn codigos_diferentes_avisa_do_risco_em_vez_de_so_pedir_para_repetir() {
        // Códigos diferentes é o sinal de homem no meio. Dizer "tente de novo" ensinaria o
        // usuário a insistir exatamente onde ele não deveria.
        let texto = Falha::CodigosDiferentes.o_que_fazer();
        assert!(
            texto.contains("meio"),
            "o risco precisa estar dito: `{texto}`"
        );
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
