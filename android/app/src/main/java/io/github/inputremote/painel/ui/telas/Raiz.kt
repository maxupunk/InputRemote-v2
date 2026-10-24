package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.inputremote.painel.dados.Arranjo
import io.github.inputremote.painel.dados.ComputadorAchado
import io.github.inputremote.painel.ui.Tela
import io.github.inputremote.painel.ui.tema.Cores
import io.github.inputremote.painel.ui.tema.TemaDoPainel

/** O que as telas pedem sobre a ligação. Tudo vai para o [io.github.inputremote.painel.ui.Controlador]. */
data class Acoes(
    val procurar: () -> Unit,
    val parear: (ComputadorAchado) -> Unit,
    val confirmar: (Boolean) -> Unit,
    val esquecer: () -> Unit,
)

/**
 * O que a pessoa escolhe neste aparelho, e como mudar. Vem das
 * [io.github.inputremote.painel.ui.Preferencias]; o protetor de tela passa um só para ler.
 */
data class Aparelho(
    val arranjo: Arranjo,
    /** Toda edição dos cards: a tela diz o que muda, por exemplo `{ it.remover(chave) }`. */
    val editarArranjo: ((Arranjo) -> Arranjo) -> Unit,
    val escurecido: Boolean,
    val alternarEscurecer: () -> Unit,
    val sempreDeitado: Boolean,
    val alternarDeitado: () -> Unit,
    /** Se dá para mexer: no protetor de tela, um toque só acorda o aparelho. */
    val editavel: Boolean = true,
)

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
