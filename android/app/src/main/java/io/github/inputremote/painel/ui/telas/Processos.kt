package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.github.inputremote.painel.dados.Arranjo
import io.github.inputremote.painel.dados.LinhaDeProcesso
import io.github.inputremote.painel.dados.ListaEstavel
import io.github.inputremote.painel.dados.Ordem
import io.github.inputremote.painel.dados.VistaDaMaquina
import io.github.inputremote.painel.ui.tema.Cores
import io.github.inputremote.painel.ui.tema.cor
import io.github.inputremote.painel.ui.tema.numeros
import kotlinx.coroutines.delay

/** Os processos que quase todo mundo quer acompanhar, a um toque. */
private val ATALHOS = listOf("docker", "wsl", "vmmem")

/** A largura do botão de cada linha: "+ Acompanhar" inteiro, e o mesmo lugar para "✓ no painel". */
private val LARGURA_DO_BOTAO = 150.dp

/** Depois de quanto tempo sem lista o painel desconfia que o computador não a manda. */
private const val PACIENCIA_MS = 6_000L

/**
 * A lista de processos de um computador, ao vivo: os mais pesados em processador e em memória,
 * somados pelo nome, a cada segundo. Ordena pela coluna escolhida, e cada linha vira um card com um
 * toque — "acompanhar".
 *
 * Ocupa a tela quase toda: é para ler de longe, como o painel.
 */
@Composable
fun ListaDeProcessos(maquina: VistaDaMaquina, ordemInicial: Ordem, aparelho: Aparelho, fechar: () -> Unit) {
    var ordem by rememberSaveable(ordemInicial) { mutableStateOf(ordemInicial) }
    var outro by remember { mutableStateOf(false) }
    val estavel = remember { ListaEstavel() }
    Dialog(onDismissRequest = fechar, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Card(
            modifier = Modifier.fillMaxSize().padding(24.dp),
            colors = CardDefaults.cardColors(containerColor = Cores.superficie),
            border = BorderStroke(1.dp, Cores.borda),
        ) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text("Processos · ${maquina.nome}", fontSize = 18.sp, fontWeight = FontWeight.SemiBold, color = Cores.texto)
                        Text("Os mais pesados, somados pelo nome. Atualiza a cada segundo.", fontSize = 13.sp, color = Cores.textoFraco)
                    }
                    TextButton(onClick = fechar) { Text("Fechar") }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Ordenar por", fontSize = 13.sp, color = Cores.textoFraco)
                    Ordem.entries.forEach { opcao ->
                        FilterChip(selected = ordem == opcao, onClick = { ordem = opcao }, label = { Text(opcao.nome) })
                    }
                    Spacer(Modifier.weight(1f))
                    OutlinedButton(onClick = { outro = true }) { Text("Acompanhar outro…") }
                }
                val linhas = maquina.processos
                if (linhas == null) {
                    Medindo()
                } else {
                    Linhas(estavel.arrumar(linhas, ordem, System.currentTimeMillis()), ordem, aparelho)
                }
            }
        }
    }
    if (outro) AcompanharProcesso(aparelho, maquina.processos.orEmpty().map { it.nome }) { outro = false }
}

/** Enquanto a primeira lista não chega — e, se não chega nunca, por quê. */
@Composable
private fun Medindo() {
    val demorou by produceState(false) {
        delay(PACIENCIA_MS)
        value = true
    }
    Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        if (demorou) {
            Text(
                "Este computador não mandou a lista. Ela precisa do InputRemote atualizado nele.",
                color = Cores.textoFraco,
                fontSize = 15.sp,
            )
        } else {
            CircularProgressIndicator(color = Cores.acao)
            Spacer(Modifier.height(12.dp))
            Text("Medindo os processos…", color = Cores.textoFraco, fontSize = 15.sp)
        }
    }
}

@Composable
private fun Linhas(linhas: List<LinhaDeProcesso>, ordem: Ordem, aparelho: Aparelho) {
    if (linhas.isEmpty()) {
        Text("Nenhum processo para mostrar.", color = Cores.textoFraco, fontSize = 15.sp)
        return
    }
    LazyColumn(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        items(linhas, key = { it.chave }) { linha -> Linha(linha, ordem, ordem.fracao(linha, linhas), aparelho) }
    }
}

/** Uma linha: o nome e quantos são, os dois números, a barra da coluna escolhida e o botão. */
@Composable
private fun Linha(linha: LinhaDeProcesso, ordem: Ordem, fracao: Float, aparelho: Aparelho) {
    val quantos = if (linha.instancias == 1) "1 processo" else "${linha.instancias} processos"
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .background(Cores.fundo)
            .padding(horizontal = 12.dp, vertical = 8.dp)
            .semantics(mergeDescendants = true) {
                contentDescription = "${linha.nome}, $quantos, processador ${linha.cpu}, memória ${linha.memoria}"
            },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    linha.nome,
                    fontSize = 16.sp,
                    color = Cores.texto,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                Text(quantos, fontSize = 12.sp, color = Cores.textoFraco, maxLines = 1)
            }
            Barra(fracao, if (ordem == Ordem.PROCESSADOR) linha.nivel.cor() else Cores.acao, 3.dp)
        }
        Column(Modifier.width(96.dp), horizontalAlignment = Alignment.End) {
            Text(ordem.valor(linha), style = numeros, fontSize = 18.sp, color = Cores.texto, maxLines = 1)
            Text(ordem.outro(linha), style = numeros, fontSize = 12.sp, color = Cores.textoFraco, maxLines = 1)
        }
        BotaoAcompanhar(linha.nome, linha.chave, aparelho)
    }
}

/** "Acompanhar": um card do processo no painel. Já acompanhado, diz; sem lugar, também. */
@Composable
private fun BotaoAcompanhar(nome: String, chave: String, aparelho: Aparelho) {
    val arranjo = aparelho.arranjo
    when {
        // Da altura do botão: a linha não encolhe quando o botão vira o aviso.
        arranjo.visivel(chave) -> Box(Modifier.width(LARGURA_DO_BOTAO).height(48.dp), contentAlignment = Alignment.CenterStart) {
            Text("✓ no painel", color = Cores.bom, fontSize = 14.sp, modifier = Modifier.padding(start = 12.dp))
        }
        !aparelho.editavel -> Spacer(Modifier.width(LARGURA_DO_BOTAO))
        else -> TextButton(
            onClick = { aparelho.editarArranjo { it.acompanhar(nome) } },
            enabled = arranjo.podeAcompanhar(),
            modifier = Modifier.width(LARGURA_DO_BOTAO),
        ) { Text(if (arranjo.podeAcompanhar()) "+ Acompanhar" else "4 no máximo", maxLines = 1) }
    }
}

/**
 * Acompanhar um processo pelo nome: um card que soma todos os processos com aquele nome — `docker`
 * pega o Docker Desktop e o motor dele; `wsl`, o WSL e a máquina virtual dele.
 *
 * @param sugestoes nomes da lista aberta, quando há, além dos atalhos.
 */
@Composable
fun AcompanharProcesso(aparelho: Aparelho, sugestoes: List<String>, fechar: () -> Unit) {
    var nome by rememberSaveable { mutableStateOf("") }
    val arranjo = aparelho.arranjo
    val acompanhar = { escolhido: String ->
        aparelho.editarArranjo { it.acompanhar(escolhido) }
        fechar()
    }
    val atalhos = (ATALHOS + sugestoes).distinct().filterNot { arranjo.visivel(Arranjo.chaveDoProcesso(it)) }.take(8)
    AlertDialog(
        onDismissRequest = fechar,
        title = { Text("Acompanhar processo") },
        confirmButton = {
            TextButton(onClick = { acompanhar(nome) }, enabled = nome.isNotBlank() && arranjo.podeAcompanhar()) {
                Text("Acompanhar")
            }
        },
        dismissButton = { TextButton(onClick = fechar) { Text("Cancelar") } },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                if (!arranjo.podeAcompanhar()) {
                    Text(
                        "Já há ${Arranjo.MAX_PROCESSOS} processos no painel. Tire um card de processo para pôr outro.",
                        color = Cores.atencao,
                        fontSize = 14.sp,
                    )
                }
                Text(
                    "O card soma todos os processos que têm este nome. \"docker\" pega o Docker inteiro.",
                    color = Cores.textoFraco,
                    fontSize = 14.sp,
                )
                OutlinedTextField(
                    value = nome,
                    onValueChange = { nome = it.take(20) },
                    label = { Text("Nome do processo") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { if (nome.isNotBlank()) acompanhar(nome) }),
                    modifier = Modifier.fillMaxWidth(),
                )
                Atalhos(atalhos, arranjo.podeAcompanhar(), acompanhar)
            }
        },
    )
}

@Composable
private fun Atalhos(nomes: List<String>, pode: Boolean, escolher: (String) -> Unit) {
    if (nomes.isEmpty()) return
    // Em linhas de três: cabe no diálogo de um celular em pé sem rolar para o lado.
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        nomes.chunked(3).forEach { linha ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                linha.forEach { nome ->
                    SuggestionChip(onClick = { escolher(nome) }, label = { Text(nome) }, enabled = pode)
                }
            }
        }
    }
}

/**
 * O "i" de um card: abre a lista de processos. Pequeno no desenho, mas com área de toque de dedo.
 */
@Composable
fun BotaoDeInformacao(descricao: String, abrir: () -> Unit) {
    Box(
        Modifier
            .size(36.dp)
            .clip(RoundedCornerShape(50))
            .clickable(onClick = abrir)
            .semantics { contentDescription = descricao },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(22.dp)
                .clip(RoundedCornerShape(50))
                .background(Cores.borda),
            contentAlignment = Alignment.Center,
        ) {
            Text("i", color = Cores.texto, fontSize = 13.sp, fontWeight = FontWeight.Bold)
        }
    }
}
