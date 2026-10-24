package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.dados.Bloco
import io.github.inputremote.painel.dados.Nivel
import io.github.inputremote.painel.ui.tema.Cores
import io.github.inputremote.painel.ui.tema.cor
import io.github.inputremote.painel.ui.tema.numeros
import io.github.inputremote.painel.ui.tema.rotulo
import kotlinx.coroutines.delay

/**
 * Um número na tela. Todo card é desenhado por este componente, qualquer que seja a métrica: o
 * texto, a barra e o nível já vêm prontos do núcleo.
 *
 * Dois tamanhos, com o mesmo cabeçalho e a mesma barra: o **destaque**, para o que se olha primeiro
 * — número grande e o último minuto em gráfico —, e o **compacto**, uma linha densa com o valor e o
 * detalhe lado a lado, para caber o resto na mesma tela deitada.
 */
@Composable
fun BlocoDeMetrica(bloco: Bloco, serie: List<Float>, destaque: Boolean, modifier: Modifier = Modifier) {
    val cor = bloco.nivel.cor()
    val descricao = listOfNotNull(bloco.rotulo, bloco.valor, bloco.detalhe, bloco.nivel.rotulo().ifEmpty { null })
        .joinToString(", ")
    val respiro = if (destaque) 14.dp else 10.dp
    Column(
        modifier = modifier
            .clip(RoundedCornerShape(12.dp))
            .background(Cores.fundo)
            .padding(horizontal = respiro, vertical = if (destaque) 11.dp else 6.dp)
            .semantics(mergeDescendants = true) { contentDescription = descricao },
        verticalArrangement = Arrangement.spacedBy(if (destaque) 4.dp else 2.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(bloco.rotulo, color = Cores.textoFraco, fontSize = 13.sp, maxLines = 1, modifier = Modifier.weight(1f))
            val alerta = bloco.nivel.rotulo()
            if (alerta.isNotEmpty()) Text(alerta, color = cor, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
        }
        if (destaque) {
            Valor(bloco, 36.sp)
            bloco.detalhe?.let { Detalhe(it) }
            bloco.fracao?.let { Barra(it, cor, 6.dp) }
            if (serie.size > 1) MiniGrafico(serie, cor, Modifier.fillMaxWidth().height(30.dp))
        } else {
            // O valor nunca é cortado: quem encolhe, com reticências, é o detalhe.
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Valor(bloco, 20.sp)
                bloco.detalhe?.let { Detalhe(it, Modifier.weight(1f, fill = false)) }
            }
            bloco.fracao?.let { Barra(it, cor, 3.dp) }
        }
    }
}

@Composable
private fun Valor(bloco: Bloco, tamanho: TextUnit, modifier: Modifier = Modifier) = Text(
    bloco.valor,
    style = numeros,
    fontSize = tamanho,
    color = if (bloco.nivel == Nivel.SEM_DADO) Cores.textoFraco else Cores.texto,
    maxLines = 1,
    modifier = modifier,
)

@Composable
private fun Detalhe(texto: String, modifier: Modifier = Modifier) = Text(
    texto,
    color = Cores.textoFraco,
    fontSize = 12.sp,
    style = numeros,
    maxLines = 1,
    overflow = TextOverflow.Ellipsis,
    modifier = modifier.padding(bottom = 2.dp),
)

/** Uma barra de 0 a 1. */
@Composable
fun Barra(fracao: Float, cor: Color, altura: Dp = 4.dp) {
    Box(
        Modifier
            .fillMaxWidth()
            .height(altura)
            .clip(RoundedCornerShape(altura / 2))
            .background(Cores.borda),
    ) {
        Box(
            Modifier
                .fillMaxWidth(fracao.coerceIn(0f, 1f))
                .height(altura)
                .background(cor),
        )
    }
}

/**
 * O último minuto de um número. A escala vai do menor ao maior da própria série, com um piso de
 * variação: um número que oscila 0,1 % não deve parecer uma montanha-russa.
 */
@Composable
fun MiniGrafico(valores: List<Float>, cor: Color, modifier: Modifier = Modifier) {
    Canvas(modifier) {
        val menor = valores.min()
        val maior = valores.max()
        val faixa = (maior - menor).coerceAtLeast(maxOf(maior * 0.1f, 1f))
        val passo = size.width / (valores.size - 1).coerceAtLeast(1)
        val caminho = Path()
        valores.forEachIndexed { i, v ->
            val ponto = Offset(i * passo, size.height - (v - menor) / faixa * size.height)
            if (i == 0) caminho.moveTo(ponto.x, ponto.y) else caminho.lineTo(ponto.x, ponto.y)
        }
        drawPath(caminho, cor.copy(alpha = 0.85f), style = Stroke(width = 2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}

/** Um ponto colorido: o nível de um cartão. */
@Composable
fun Ponto(cor: Color, tamanho: Dp = 10.dp) {
    Box(Modifier.size(tamanho).clip(RoundedCornerShape(50)).background(cor))
}

/**
 * Os seis dígitos, grandes e em três e três, como na janela do computador: quem compara lê em voz
 * alta, e "104 782" se lê certo onde "104782" se lê errado.
 */
@Composable
fun Digitos(digitos: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        digitos.forEachIndexed { i, digito ->
            if (i == 3) Spacer(Modifier.width(12.dp))
            Box(
                Modifier
                    .size(width = 48.dp, height = 64.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .background(MaterialTheme.colorScheme.surface),
                contentAlignment = Alignment.Center,
            ) {
                Text(digito.toString(), style = numeros, fontSize = 32.sp, color = Cores.texto)
            }
        }
    }
}

/**
 * O conteúdo inteiro se desloca alguns pontos a cada minuto: num OLED aceso o dia todo, um texto
 * parado no mesmo lugar marca a tela (docs/12, §5.3). Pouco o bastante para ninguém notar.
 */
@Composable
fun Deslocado(conteudo: @Composable () -> Unit) {
    val passo by produceState(0) {
        while (true) {
            delay(60_000)
            value = (value + 1) % DESLOCAMENTOS.size
        }
    }
    val (x, y) = DESLOCAMENTOS[passo]
    Box(Modifier.offset(x.dp, y.dp)) { conteudo() }
}

private val DESLOCAMENTOS = listOf(0 to 0, 2 to 1, 3 to -1, 1 to -2, -1 to -2, -3 to 0, -2 to 2, 0 to 3)
