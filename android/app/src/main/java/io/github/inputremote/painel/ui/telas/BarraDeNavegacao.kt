package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.ui.tema.Cores

/** Rótulos fixos das páginas utilitárias após os painéis personalizados. */
object PaginasFixas {
    const val PROCESSOS = "Processos"
    const val CONEXOES = "Conexões"
    const val AJUSTES = "Ajustes"

    val TODAS = listOf(PROCESSOS, CONEXOES, AJUSTES)
}

/**
 * Barra de abas em formato pílula (segmented control) com suporte a rolagem horizontal
 * para acomodar múltiplos painéis personalizados sem apertar a barra.
 */
@Composable
fun BarraDeNavegacao(
    titulos: List<String>,
    paginaAtual: Int,
    aoMudarPagina: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier
            .clip(RoundedCornerShape(20.dp))
            .background(Cores.superficie)
            .padding(2.dp)
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        titulos.forEachIndexed { indice, rotulo ->
            val selecionada = paginaAtual == indice
            val corFundo = if (selecionada) Cores.borda else Color.Transparent
            val corTexto = if (selecionada) Cores.texto else Cores.textoFraco
            val pesoFonte = if (selecionada) FontWeight.SemiBold else FontWeight.Normal
            Box(
                modifier = Modifier
                    .clip(RoundedCornerShape(16.dp))
                    .background(corFundo)
                    .clickable { aoMudarPagina(indice) }
                    .padding(horizontal = 12.dp, vertical = 5.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = rotulo,
                    color = corTexto,
                    fontSize = 12.sp,
                    fontWeight = pesoFonte,
                    maxLines = 1,
                )
            }
        }
    }
}
