//! O que o serviço conta à interface sem ela pedir: o código de pareamento, uma mudança de estado,
//! uma transferência.
//!
//! Saiu de [`crate::ui`] pelo limite de 400 linhas por arquivo, quando o painel no Android entrou.

use serde::{Deserialize, Serialize};

use crate::status::Estado;
use crate::ui::Candidato;

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
    /// Uma ação que o serviço terminou em segundo plano não deu certo.
    ///
    /// O pedido já tinha respondido "feito" — desligar a economia do Wi-Fi roda uma ferramenta do
    /// sistema, fora do laço —, e sem isto a falha só ia para o registro: o botão parecia
    /// funcionar e nada mudava.
    Falhou(crate::Falha),
    /// O outro computador mudou de lado na tela dele, e este passou a usar a borda oposta.
    ///
    /// A borda já muda no [`Estado`]; isto é para a tela **contar** — uma posição que muda sem
    /// ninguém tocar parece defeito.
    BordaAjustada(crate::Borda),
    /// Uma cópia de arquivos começou a chegar: o ajudante de clipboard já pode prometê-la.
    ///
    /// No fim do enum, como toda variante nova: o canal local é `postcard`, que numera as variantes
    /// pela posição, e um processo de antes de uma atualização ainda lê os números antigos.
    ArquivosChegando(crate::chegada::Chegando),
    /// As pastas compartilhadas mudaram — uma nova, uma oferta, uma que sincronizou ou entrou em
    /// conflito. Leva a lista inteira, como o estado.
    PastasMudaram(Vec<crate::pastas::ResumoDePasta>),
    /// Um pedido sobre as pastas não deu certo: a frase do ajudante, já com o que fazer.
    RecadoDasPastas(String),
    /// O outro computador copiou arquivos de uma pasta compartilhada: estes caminhos, na cópia
    /// daqui da pasta, vão para o clipboard. Só para o ajudante de clipboard.
    ArquivosDaPasta(Vec<String>),
    /// O outro computador ligou ou desligou "copiar e colar", e este acompanhou: a escolha vale
    /// para os dois. Como [`Self::BordaAjustada`], para a tela **contar** — o Ctrl+C que para de
    /// atravessar sem ninguém mexer aqui parece defeito.
    CopiarEColarAjustado {
        /// Como ficou.
        ligado: bool,
        /// O nome do outro computador, como ele se anuncia.
        par: String,
    },
    /// Algo sobre os painéis no Android: o código a conferir, o fim do pareamento, a lista.
    Painel(crate::paineis::AvisoDoPainel),
}

impl Aviso {
    /// Se este aviso é só para o ajudante de clipboard — o conteúdo do clipboard, que a janela não
    /// usa. Os de arquivos levam caminhos desta máquina e podem ter centenas de quilobytes.
    #[must_use]
    pub const fn so_para_o_ajudante(&self) -> bool {
        matches!(
            self,
            Self::LerClipboard
                | Self::TextoRecebido(_)
                | Self::ArquivosChegando(_)
                | Self::ArquivosDaPasta(_)
        )
    }
}
