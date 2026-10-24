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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.dados.Arranjo
import io.github.inputremote.painel.dados.Bloco
import io.github.inputremote.painel.dados.Historico
import io.github.inputremote.painel.dados.Nivel
import io.github.inputremote.painel.dados.Ordem
import io.github.inputremote.painel.dados.Vista
import io.github.inputremote.painel.dados.VistaDaMaquina
import io.github.inputremote.painel.dados.VistaDoEnlace
import io.github.inputremote.painel.ui.Tela
import io.github.inputremote.painel.ui.tema.Cores
import io.github.inputremote.painel.ui.tema.cor

/**
 * A largura mínima de um card destacado: "100 %" a 40 sp cabe, e numa coluna de tablet deitado
 * (~380 dp no de 8") o processador e a memória ficam lado a lado.
 */
private val LARGURA_DO_DESTAQUE = 170.dp

/** A largura mínima de um card compacto. */
private val LARGURA_DO_COMPACTO = 140.dp

/** O nome da faixa do enlace, quando ela é oferecida em "Adicionar card". */
private const val NOME_DO_ENLACE = "Entre os computadores"

/** O nome da faixa da cópia, idem. */
private const val NOME_DA_COPIA = "Cópia de arquivos"

/** Os cards cujo "i" abre a lista de processos. */
private val COM_LISTA = setOf("cpu", "ram")

/**
 * O painel, pensado deitado: um computador por coluna, o processador e a memória grandes no topo
 * de cada uma, o resto numa grade compacta, e a faixa do enlace fina embaixo. Em pé, os
 * computadores se empilham e a faixa vai para o topo (docs/12, §5.2). Quais cards aparecem, e em
 * que ordem, é do [Aparelho.arranjo]. O "i" do processador e da memória abre a lista de processos
 * daquele computador; a cópia de arquivos aparece numa faixa enquanto acontece.
 */
@Composable
fun TelaDoPainel(painel: Tela.Painel, acoes: Acoes, aparelho: Aparelho) {
    var editando by rememberSaveable { mutableStateOf(false) }
    var ordem by rememberSaveable { mutableStateOf(Ordem.PROCESSADOR) }
    val informar: ((Int, String) -> Unit)? = if (aparelho.editavel) {
        { maquina, chave ->
            ordem = Ordem.doCard(chave)
            acoes.abrirLista(maquina)
        }
    } else {
        null
    }
    val rotulos = remember(painel.vista) { rotulosDa(painel.vista) }
    // Uma métrica que este aparelho nunca viu entra no fim, visível: a versão nova do computador
    // não fica escondida esperando alguém ir procurá-la.
    LaunchedEffect(rotulos.keys) { aparelho.editarArranjo { it.conhecer(rotulos.keys) } }

    BoxWithConstraints(Modifier.fillMaxSize().padding(12.dp)) {
        val deitado = maxWidth > maxHeight
        val enlace = painel.vista.enlace?.takeIf { aparelho.arranjo.visivel(Arranjo.ENLACE) }
        val copia = aparelho.arranjo.visivel(Arranjo.COPIA) && (painel.vista.copia != null || editando)
        // Deitado, as duas faixas dividem uma linha: a altura é dos cartões dos computadores.
        val faixas: @Composable () -> Unit = {
            if (deitado) {
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    if (enlace != null) Box(Modifier.weight(1.4f)) { FaixaDoEnlace(enlace, painel.series, editando, aparelho) }
                    if (copia) Box(Modifier.weight(1f)) { FaixaDaCopia(painel.vista.copia, painel.series, editando, aparelho) }
                }
            } else {
                if (enlace != null) FaixaDoEnlace(enlace, painel.series, editando, aparelho)
                if (copia) FaixaDaCopia(painel.vista.copia, painel.series, editando, aparelho)
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Cabecalho(painel, acoes, aparelho) { editando = true }
            if (editando) BarraDeEdicao(aparelho, rotulos) { editando = false }
            Column(
                modifier = Modifier.weight(1f).alpha(if (painel.segundosSemDados > 0) 0.45f else 1f),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                if (!deitado) faixas()
                Maquinas(painel, aparelho, editando, lado = deitado, modifier = Modifier.weight(1f), informar)
                if (deitado) faixas()
            }
        }
    }
    val lista = painel.lista?.let { painel.vista.maquinas.getOrNull(it) }
    if (lista != null) ListaDeProcessos(lista, ordem, aparelho) { acoes.abrirLista(null) }
}

/**
 * O nome de cada card que existe agora, pela chave. A cópia está sempre: ela só aparece quando há
 * cópia, mas a pessoa precisa poder pô-la e tirá-la a qualquer hora.
 */
private fun rotulosDa(vista: Vista): Map<String, String> {
    val blocos = vista.maquinas.flatMap { it.blocos }.associate { it.chave to it.rotulo } + (Arranjo.COPIA to NOME_DA_COPIA)
    return if (vista.enlace == null) blocos else blocos + (Arranjo.ENLACE to NOME_DO_ENLACE)
}

@Composable
private fun Cabecalho(painel: Tela.Painel, acoes: Acoes, aparelho: Aparelho, editar: () -> Unit) {
    val velho = painel.segundosSemDados > 0
    Row(verticalAlignment = Alignment.CenterVertically) {
        Ponto(if (velho) Cores.atencao else Cores.bom)
        Spacer(Modifier.width(8.dp))
        Text(
            if (velho) "${painel.computador} · sem dados há ${painel.segundosSemDados} s" else painel.computador,
            color = if (velho) Cores.atencao else Cores.textoFraco,
            fontSize = 14.sp,
            modifier = Modifier.weight(1f),
        )
        if (aparelho.editavel) Menu(acoes, aparelho, editar)
    }
}

@Composable
private fun Menu(acoes: Acoes, aparelho: Aparelho, editar: () -> Unit) {
    var aberto by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { aberto = true }) { Text("⋮", fontSize = 20.sp, color = Cores.textoFraco) }
        DropdownMenu(expanded = aberto, onDismissRequest = { aberto = false }) {
            val item: @Composable (String, () -> Unit) -> Unit = { texto, acao ->
                DropdownMenuItem(text = { Text(texto) }, onClick = { aberto = false; acao() })
            }
            item("Editar painel", editar)
            item(if (aparelho.sempreDeitado) "Girar com o aparelho" else "Sempre deitado", aparelho.alternarDeitado)
            item(if (aparelho.escurecido) "Tela no brilho normal" else "Escurecer a tela", aparelho.alternarEscurecer)
            item("Parear outro computador", acoes.procurar)
            item("Esquecer este computador", acoes.esquecer)
        }
    }
}

@Composable
private fun Maquinas(
    painel: Tela.Painel,
    aparelho: Aparelho,
    editando: Boolean,
    lado: Boolean,
    modifier: Modifier,
    informar: ((Int, String) -> Unit)?,
) {
    val maquinas = painel.vista.maquinas
    // O "i" de cada cartão abre a lista daquele computador: a posição dele no quadro.
    val deCada = { posicao: Int -> informar?.let { abrir -> { chave: String -> abrir(posicao, chave) } } }
    if (lado && maquinas.size > 1) {
        Row(modifier, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            maquinas.forEachIndexed { i, maquina ->
                val mod = Modifier.weight(1f).fillMaxHeight()
                CartaoDaMaquina(maquina, painel.series, aparelho, editando, mod, rolavel = true, informar = deCada(i))
            }
        }
    } else {
        Column(modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            maquinas.forEachIndexed { i, maquina ->
                CartaoDaMaquina(maquina, painel.series, aparelho, editando, Modifier.fillMaxWidth(), informar = deCada(i))
            }
        }
    }
}

/**
 * Um computador: o nome, os cards destacados grandes e o resto compacto, na ordem do arranjo. O que
 * não cabe rola dentro do cartão — deitado, ele tem a altura da tela.
 */
@Composable
private fun CartaoDaMaquina(
    maquina: VistaDaMaquina,
    series: Map<String, List<Float>>,
    aparelho: Aparelho,
    editando: Boolean,
    modifier: Modifier,
    rolavel: Boolean = false,
    informar: ((String) -> Unit)? = null,
) {
    val borda = if (maquina.nivel == Nivel.ATENCAO || maquina.nivel == Nivel.CRITICO) maquina.nivel.cor() else Cores.borda
    val (grandes, compactos) = aparelho.arranjo.ordenar(maquina.blocos) { it.chave }.partition { it.second.destaque }
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, borda),
    ) {
        // Só rola por dentro lado a lado: empilhados, quem rola é a coluna — rolagem dentro de rolagem
        // não tem altura para medir.
        val rolagem = if (rolavel) Modifier.verticalScroll(rememberScrollState()) else Modifier
        Column(
            rolagem.padding(10.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(maquina.nome, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = Cores.texto)
            maquina.aviso?.let { Text(it, color = Cores.textoFraco, fontSize = 15.sp) }
            val desenhar: @Composable (Bloco, Boolean, Modifier) -> Unit = { bloco, destaque, mod ->
                Editavel(bloco.chave, editando, aparelho, mod) {
                    val serie = series[Historico.chave(maquina.nome, bloco.chave)].orEmpty()
                    val abrir = informar?.takeIf { !editando && bloco.chave in COM_LISTA }
                    BlocoDeMetrica(bloco, serie, destaque, Modifier.fillMaxWidth(), abrir?.let { { it(bloco.chave) } })
                }
            }
            Grade(grandes.map { it.first }, LARGURA_DO_DESTAQUE) { bloco, mod -> desenhar(bloco, true, mod) }
            Grade(compactos.map { it.first }, LARGURA_DO_COMPACTO) { bloco, mod -> desenhar(bloco, false, mod) }
        }
    }
}

/** A faixa do enlace: fina, numa linha — a volta de cada rota e qual está ganhando. */
@Composable
private fun FaixaDoEnlace(enlace: VistaDoEnlace, series: Map<String, List<Float>>, editando: Boolean, aparelho: Aparelho) {
    Editavel(Arranjo.ENLACE, editando, aparelho, podeMover = false, podeDestacar = false) {
        Card(colors = CardDefaults.cardColors(containerColor = Cores.superficie), border = BorderStroke(1.dp, Cores.borda)) {
            Row(
                Modifier.padding(6.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                enlace.rotas.forEach { bloco ->
                    val serie = series[Historico.chave(Historico.ENLACE, bloco.chave)].orEmpty()
                    BlocoDeMetrica(bloco, serie, destaque = false, Modifier.weight(1f))
                }
                Column(Modifier.width(180.dp).padding(horizontal = 4.dp)) {
                    Text(NOME_DO_ENLACE, color = Cores.textoFraco, fontSize = 12.sp, maxLines = 1)
                    enlace.vencendo?.let { Text("mais rápido: $it", color = Cores.acao, fontSize = 13.sp, maxLines = 1) }
                }
            }
        }
    }
}

/**
 * A cópia de arquivos entre os computadores, numa faixa fina como a do enlace: o que vai, quanto
 * já foi, a velocidade e quanto falta. Some sozinha um pouco depois de a cópia acabar; na edição,
 * fica um lugar marcado, para a pessoa poder tirá-la sem esperar uma cópia.
 */
@Composable
private fun FaixaDaCopia(copia: Bloco?, series: Map<String, List<Float>>, editando: Boolean, aparelho: Aparelho) {
    Editavel(Arranjo.COPIA, editando, aparelho, podeMover = false, podeDestacar = false) {
        Card(colors = CardDefaults.cardColors(containerColor = Cores.superficie), border = BorderStroke(1.dp, Cores.borda)) {
            if (copia == null) {
                Text(
                    "$NOME_DA_COPIA: aparece aqui enquanto uma cópia acontece",
                    color = Cores.textoFraco,
                    fontSize = 13.sp,
                    modifier = Modifier.fillMaxWidth().padding(12.dp),
                )
            } else {
                val serie = series[Historico.chave(Historico.ENLACE, copia.chave)].orEmpty()
                BlocoDeMetrica(copia, serie, destaque = false, Modifier.fillMaxWidth().padding(6.dp))
            }
        }
    }
}

/** Cards em linhas, quantos couberem na largura, todos do mesmo tamanho. */
@Composable
private fun Grade(blocos: List<Bloco>, largura: Dp, item: @Composable (Bloco, Modifier) -> Unit) {
    if (blocos.isEmpty()) return
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val porLinha = (maxWidth / largura).toInt().coerceIn(1, 4)
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
