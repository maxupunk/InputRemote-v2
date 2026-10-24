package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.inputremote.painel.dados.ComputadorAchado
import io.github.inputremote.painel.ui.Tela
import io.github.inputremote.painel.ui.tema.Cores
import io.github.inputremote.painel.ui.tema.TemaDoPainel

/** O que as telas podem pedir. Tudo vai para o [io.github.inputremote.painel.ui.Controlador]. */
data class Acoes(
    val procurar: () -> Unit,
    val parear: (ComputadorAchado) -> Unit,
    val confirmar: (Boolean) -> Unit,
    val esquecer: () -> Unit,
    /** Se a tela está escurecida agora. */
    val escurecido: Boolean,
    val alternarEscurecer: () -> Unit,
)

/** A tela do momento, com o deslocamento contra marca na tela. */
@Composable
fun Painel(tela: Tela, acoes: Acoes) {
    TemaDoPainel {
        Box(Modifier.fillMaxSize().background(Cores.fundo).safeDrawingPadding()) {
            Deslocado {
                when (tela) {
                    Tela.Abrindo -> TelaDeAbertura()
                    is Tela.Procurando -> TelaDeProcura(tela, acoes)
                    is Tela.Conferindo -> TelaDeConferencia(tela, acoes)
                    is Tela.Conectando -> TelaDeConexao(tela, acoes)
                    is Tela.Painel -> TelaDoPainel(tela, acoes)
                }
            }
        }
    }
}
