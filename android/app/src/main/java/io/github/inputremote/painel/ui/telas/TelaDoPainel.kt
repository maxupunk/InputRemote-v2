package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.dados.Bloco
import io.github.inputremote.painel.dados.Historico
import io.github.inputremote.painel.dados.Nivel
import io.github.inputremote.painel.dados.VistaDaMaquina
import io.github.inputremote.painel.dados.VistaDoEnlace
import io.github.inputremote.painel.ui.Tela
import io.github.inputremote.painel.ui.tema.Cores
import io.github.inputremote.painel.ui.tema.cor

/** A largura mínima de um bloco: abaixo dela o número grande não cabe. */
private val LARGURA_DO_BLOCO = 160.dp

/**
 * O painel. Tablet deitado: um computador por coluna, a faixa do enlace embaixo. Em pé: um embaixo
 * do outro, com a faixa no topo (docs/12, §5.2).
 */
@Composable
fun TelaDoPainel(painel: Tela.Painel, acoes: Acoes) {
    BoxWithConstraints(Modifier.fillMaxSize().padding(16.dp)) {
        val deitado = maxWidth > maxHeight
        val velho = painel.segundosSemDados > 0
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Cabecalho(painel, acoes)
            Column(
                modifier = Modifier.weight(1f).alpha(if (velho) 0.45f else 1f),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                val enlace = painel.vista.enlace
                if (!deitado && enlace != null) FaixaDoEnlace(enlace, painel.series)
                Maquinas(painel, lado = deitado, modifier = Modifier.weight(1f))
                if (deitado && enlace != null) FaixaDoEnlace(enlace, painel.series)
            }
        }
    }
}

@Composable
private fun Cabecalho(painel: Tela.Painel, acoes: Acoes) {
    var menu by remember { mutableStateOf(false) }
    Row(verticalAlignment = Alignment.CenterVertically) {
        val velho = painel.segundosSemDados > 0
        Ponto(if (velho) Cores.atencao else Cores.bom)
        Spacer(Modifier.padding(start = 8.dp))
        Text(
            if (velho) "${painel.computador} · sem dados há ${painel.segundosSemDados} s" else painel.computador,
            color = if (velho) Cores.atencao else Cores.textoFraco,
            fontSize = 14.sp,
            modifier = Modifier.weight(1f),
        )
        Box {
            TextButton(onClick = { menu = true }) { Text("⋮", fontSize = 20.sp, color = Cores.textoFraco) }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(
                    text = { Text(if (acoes.escurecido) "Tela no brilho normal" else "Escurecer a tela") },
                    onClick = { menu = false; acoes.alternarEscurecer() },
                )
                DropdownMenuItem(text = { Text("Parear outro computador") }, onClick = { menu = false; acoes.procurar() })
                DropdownMenuItem(text = { Text("Esquecer este computador") }, onClick = { menu = false; acoes.esquecer() })
            }
        }
    }
}

@Composable
private fun Maquinas(painel: Tela.Painel, lado: Boolean, modifier: Modifier) {
    val maquinas = painel.vista.maquinas
    if (lado && maquinas.size > 1) {
        Row(modifier, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            // Lado a lado o cartão tem a altura da tela, e o que não couber rola dentro dele: sem
            // isso a bateria e a temperatura ficavam cortadas embaixo, no tablet deitado.
            maquinas.forEach {
                CartaoDaMaquina(it, painel.series, Modifier.weight(1f).fillMaxHeight(), rolavel = true)
            }
        }
    } else {
        Column(
            modifier.verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            maquinas.forEach { CartaoDaMaquina(it, painel.series, Modifier.fillMaxWidth()) }
        }
    }
}

/** Um computador: o nome, e os blocos em linhas que cabem na largura do cartão. */
@Composable
private fun CartaoDaMaquina(
    maquina: VistaDaMaquina,
    series: Map<String, List<Float>>,
    modifier: Modifier,
    rolavel: Boolean = false,
) {
    val borda = if (maquina.nivel == Nivel.ATENCAO || maquina.nivel == Nivel.CRITICO) maquina.nivel.cor() else Cores.borda
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, borda),
    ) {
        val rolagem = if (rolavel) Modifier.verticalScroll(rememberScrollState()) else Modifier
        Column(rolagem.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(maquina.nome, fontSize = 18.sp, fontWeight = FontWeight.SemiBold, color = Cores.texto)
            maquina.aviso?.let { Text(it, color = Cores.textoFraco, fontSize = 15.sp) }
            Grade(maquina.blocos) { bloco, mod ->
                BlocoDeMetrica(bloco, series[Historico.chave(maquina.nome, bloco.chave)].orEmpty(), mod)
            }
        }
    }
}

/** A faixa do enlace: a volta de cada rota, e qual está ganhando. */
@Composable
private fun FaixaDoEnlace(enlace: VistaDoEnlace, series: Map<String, List<Float>>) {
    Card(colors = CardDefaults.cardColors(containerColor = Cores.superficie), border = BorderStroke(1.dp, Cores.borda)) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Entre os computadores", color = Cores.textoFraco, fontSize = 13.sp, modifier = Modifier.weight(1f))
                enlace.vencendo?.let { Text("mais rápido: $it", color = Cores.acao, fontSize = 13.sp) }
            }
            Grade(enlace.rotas) { bloco, mod ->
                BlocoDeMetrica(bloco, series[Historico.chave(Historico.ENLACE, bloco.chave)].orEmpty(), mod)
            }
        }
    }
}

/** Blocos em linhas, quantos couberem na largura, todos do mesmo tamanho. */
@Composable
private fun Grade(blocos: List<Bloco>, item: @Composable (Bloco, Modifier) -> Unit) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val porLinha = (maxWidth / LARGURA_DO_BLOCO).toInt().coerceIn(1, 4)
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            blocos.chunked(porLinha).forEach { linha ->
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    linha.forEach { item(it, Modifier.weight(1f)) }
                    repeat(porLinha - linha.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }
}
