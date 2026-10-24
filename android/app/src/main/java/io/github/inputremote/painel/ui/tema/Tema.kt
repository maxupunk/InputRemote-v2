package io.github.inputremote.painel.ui.tema

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.dados.Nivel

/**
 * As cores do painel. As mesmas da janela do InputRemote no modo escuro
 * (`crates/ir-ui/ui/tema.slint`): quem olha as duas telas vê um produto só.
 */
object Cores {
    // Preto, e não o #16161A da janela: num OLED preto é pixel apagado, e o painel fica aceso o dia todo.
    val fundo = Color(0xFF000000)
    val superficie = Color(0xFF1E1E25)
    val borda = Color(0xFF2E2E38)
    val texto = Color(0xFFF2F2F5)
    val textoFraco = Color(0xFF9C9CA8)
    val acao = Color(0xFF5B8DFA)
    val bom = Color(0xFF3DDC97)
    val atencao = Color(0xFFF7B955)
    val ruim = Color(0xFFF87171)
}

/** A cor de um nível. Ao lado dela vai sempre o nome do nível — a cor sozinha não basta a todos. */
fun Nivel.cor(): Color = when (this) {
    Nivel.NORMAL -> Cores.bom
    Nivel.ATENCAO -> Cores.atencao
    Nivel.CRITICO -> Cores.ruim
    Nivel.SEM_DADO -> Cores.textoFraco
}

/** O que se lê ao lado da cor, para quem não distingue as cores. Vazio no normal. */
fun Nivel.rotulo(): String = when (this) {
    Nivel.ATENCAO -> "atenção"
    Nivel.CRITICO -> "crítico"
    Nivel.NORMAL, Nivel.SEM_DADO -> ""
}

/**
 * Os números com dígitos de largura fixa: o valor muda a cada segundo, e com dígitos proporcionais
 * o texto tremeria de um lado para o outro (docs/12, §5.2).
 */
val numeros = TextStyle(fontFeatureSettings = "tnum", fontWeight = FontWeight.SemiBold)

private val esquema = darkColorScheme(
    primary = Cores.acao,
    onPrimary = Color.White,
    background = Cores.fundo,
    onBackground = Cores.texto,
    surface = Cores.superficie,
    onSurface = Cores.texto,
    onSurfaceVariant = Cores.textoFraco,
    outline = Cores.borda,
    error = Cores.ruim,
)

/** O tema do painel: escuro sempre. Num painel aceso o dia inteiro, claro seria um farol. */
@Composable
fun TemaDoPainel(conteudo: @Composable () -> Unit) {
    MaterialTheme(colorScheme = esquema, content = conteudo)
}

/** O tamanho do número principal de um bloco. */
val tamanhoDoValor = 26.sp
