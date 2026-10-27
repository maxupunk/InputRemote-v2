package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.inputremote.painel.dados.Arranjo
import io.github.inputremote.painel.dados.ComputadorAchado
import io.github.inputremote.painel.dados.PainelPersonalizado
import io.github.inputremote.painel.ui.Tela
import io.github.inputremote.painel.ui.tema.Cores
import io.github.inputremote.painel.ui.tema.TemaDoPainel

/** O que as telas pedem sobre a ligação. Tudo vai para o [io.github.inputremote.painel.ui.Controlador]. */
data class Acoes(
    val procurar: () -> Unit,
    val parear: (ComputadorAchado) -> Unit,
    val confirmar: (Boolean) -> Unit,
    val esquecer: () -> Unit,
    /** Abre a lista de processos do computador nesta posição do quadro, ou a fecha (nulo). */
    val abrirLista: (Int?) -> Unit = {},
    /** Informa os processos que devem ser acompanhados pelo computador. */
    val acompanhar: (List<String>) -> Unit = {},
    /** Notifica a pesquisa de processo ativa na tela de processos para acompanhamento dinâmico. */
    val pesquisarProcesso: (String) -> Unit = {},
)

/**
 * O que a pessoa escolhe neste aparelho, e como mudar. Vem das
 * [io.github.inputremote.painel.ui.Preferencias]; o protetor de tela passa um só para ler.
 */
data class Aparelho(
    /** Toda edição dos cards do painel ativo: a tela diz o que muda. */
    val editarPainel: ((PainelPersonalizado) -> PainelPersonalizado) -> Unit,
    val escurecido: Boolean,
    val alternarEscurecer: () -> Unit,
    val sempreDeitado: Boolean,
    val alternarDeitado: () -> Unit,
    /** Se dá para mexer: no protetor de tela, um toque só acorda o aparelho. */
    val editavel: Boolean = true,
    /** Painéis personalizados configurados no aparelho. */
    val paineis: List<PainelPersonalizado> = listOf(PainelPersonalizado.PADRAO),
    val painelAtivoId: String = "painel_principal",
    val aoSelecionarPainel: (String) -> Unit = {},
    val aoCriarPainel: (String) -> Unit = {},
    val aoRenomearPainel: (String, String) -> Unit = { _, _ -> },
    val aoExcluirPainel: (String) -> Unit = {},
    val aoTrocarLadoMaquinas: (String, List<String>) -> Unit = { _, _ -> },
) {
    val painelAtivo: PainelPersonalizado
        get() = paineis.firstOrNull { it.id == painelAtivoId } ?: paineis.firstOrNull() ?: PainelPersonalizado.PADRAO

    /** Os cards deste computador no painel ativo; [maquina] nulo, os do painel (as faixas). */
    fun arranjoDe(maquina: String?): Arranjo = painelAtivo.arranjoDe(maquina)

    /** Muda os cards de um computador — ou os do painel, com [maquina] nulo — por exemplo `{ it.remover(chave) }`. */
    fun editarArranjo(maquina: String?, edicao: (Arranjo) -> Arranjo) = editarPainel { it.editar(maquina, edicao) }
}

/** A tela do momento, com o deslocamento contra marca na tela. */
@Composable
fun Painel(tela: Tela, acoes: Acoes, aparelho: Aparelho) {
    TemaDoPainel {
        Box(Modifier.fillMaxSize().background(Cores.fundo).safeDrawingPadding()) {
            Deslocado {
                when (tela) {
                    Tela.Abrindo -> TelaDeAbertura()
                    is Tela.Procurando -> TelaDeProcura(tela, acoes)
                    is Tela.Conferindo -> TelaDeConferencia(tela, acoes)
                    is Tela.Conectando -> TelaDeConexao(tela, acoes)
                    is Tela.Painel -> TelaDoPainel(tela, acoes, aparelho)
                }
            }
        }
    }
}
