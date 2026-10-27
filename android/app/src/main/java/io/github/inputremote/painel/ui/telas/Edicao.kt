package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.dados.Arranjo
import io.github.inputremote.painel.ui.tema.Cores

/**
 * Um card em edição: o card, com a borda de ação, o ✕ num selo no canto — sempre à vista, por menor
 * que o card seja — e, embaixo, andar para antes, destacar e andar para depois, repartindo a largura
 * que houver. Fora da edição, só o card.
 *
 * [podeMover] e [podeDestacar] existem para a faixa do enlace, que não entra na grade dos cards: ela
 * só sai e volta.
 */
@Composable
fun Editavel(
    chave: String,
    editando: Boolean,
    aparelho: Aparelho,
    modifier: Modifier = Modifier,
    podeMover: Boolean = true,
    podeDestacar: Boolean = true,
    conteudo: @Composable () -> Unit,
) {
    if (!editando) {
        Box(modifier) { conteudo() }
        return
    }
    val destaque = aparelho.painelAtivo.arranjo.cards.firstOrNull { it.chave == chave }?.destaque == true
    val editar = aparelho.editarArranjo
    Box(modifier.border(BorderStroke(1.dp, Cores.acao), RoundedCornerShape(12.dp))) {
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            // A faixa ocupa a altura que tem: o conteúdo cede lugar à linha dos controles.
            Box(Modifier.weight(1f, fill = false)) { conteudo() }
            if (podeMover || podeDestacar) {
                Row(Modifier.fillMaxWidth().height(40.dp)) {
                    if (podeMover) Controle("◀", "Mover para antes") { editar { it.mover(chave, -1) } }
                    if (podeDestacar) {
                        Controle(if (destaque) "★" else "☆", if (destaque) "Deixar compacto" else "Destacar") {
                            editar { it.alternarDestaque(chave) }
                        }
                    }
                    if (podeMover) Controle("▶", "Mover para depois") { editar { it.mover(chave, 1) } }
                }
            }
        }
        Tirar(Modifier.align(Alignment.TopEnd)) { editar { it.remover(chave) } }
    }
}

/** Um botão de edição: reparte a linha com os outros, e diz o nome ao leitor de tela. */
@Composable
private fun RowScope.Controle(simbolo: String, nome: String, acao: () -> Unit) {
    Box(
        Modifier
            .weight(1f)
            .fillMaxHeight()
            .clip(RoundedCornerShape(8.dp))
            .clickable(onClick = acao)
            .semantics { contentDescription = nome },
        contentAlignment = Alignment.Center,
    ) {
        Text(simbolo, color = Cores.acao, fontSize = 20.sp)
    }
}

/**
 * O ✕ de tirar: um selo vermelho dentro do canto do card — dentro, para nenhuma rolagem cortá-lo —,
 * com a área de toque maior que o desenho. Tirar é seguro: "Desfazer" devolve, e "Adicionar card" também.
 */
@Composable
private fun Tirar(modifier: Modifier, acao: () -> Unit) {
    Box(
        modifier
            .size(40.dp)
            .clickable(onClick = acao)
            .semantics { contentDescription = "Tirar da tela" },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier.size(24.dp).clip(CircleShape).background(Cores.ruim),
            contentAlignment = Alignment.Center,
        ) {
            Text("✕", color = Cores.fundo, fontSize = 13.sp, fontWeight = FontWeight.Bold)
        }
    }
}

/**
 * O "Desfazer" da edição: cada mudança que muda algo guarda o arranjo de antes, com o painel dele.
 * Desfaz um passo por vez, só no painel à vista; ao concluir a edição, esquece tudo.
 */
class Desfazer {
    private val antes = mutableStateListOf<Pair<String, Arranjo>>()

    /** O editar de fora, que só salva: desfazer não é uma mudança a desfazer. */
    private var salvar: ((Arranjo) -> Arranjo) -> Unit = {}

    fun podeEm(painel: String): Boolean = antes.any { it.first == painel }

    /** O mesmo aparelho, mas cada edição passa por aqui antes. */
    fun guardando(aparelho: Aparelho): Aparelho {
        salvar = aparelho.editarArranjo
        return aparelho.copy(
            editarArranjo = { edicao ->
                val painel = aparelho.painelAtivo
                if (edicao(painel.arranjo) != painel.arranjo) antes.add(painel.id to painel.arranjo)
                aparelho.editarArranjo(edicao)
            },
        )
    }

    fun desfazer(painel: String) {
        val ultimo = antes.indexOfLast { it.first == painel }
        if (ultimo < 0) return
        val arranjo = antes.removeAt(ultimo).second
        salvar { arranjo }
    }

    fun esquecer() = antes.clear()
}

/**
 * A barra da edição: gerenciar abas de painéis (criar, renomear, excluir),
 * inverter lados das máquinas, adicionar cards, acompanhar processos e concluir.
 */
@Composable
fun BarraDeEdicao(
    aparelho: Aparelho,
    rotulos: Map<String, String>,
    aoInverterLados: (() -> Unit)? = null,
    desfazer: Desfazer? = null,
    concluir: () -> Unit,
) {
    var escolhendo by remember { mutableStateOf(false) }
    var processo by remember { mutableStateOf(false) }
    var renomeando by remember { mutableStateOf(false) }
    var criandoPainel by remember { mutableStateOf(false) }
    var confirmandoExclusao by remember { mutableStateOf(false) }

    val painelAtual = aparelho.painelAtivo

    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Editando \"${painelAtual.nome}\": ◀ ▶ mudam o lugar, ★ destaca, ✕ no canto tira.",
                color = Cores.textoFraco,
                fontSize = 12.sp,
                modifier = Modifier.weight(1f, fill = false),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                if (desfazer != null && desfazer.podeEm(painelAtual.id)) {
                    OutlinedButton(
                        onClick = { desfazer.desfazer(painelAtual.id) },
                        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 2.dp),
                    ) {
                        Text("Desfazer", fontSize = 12.sp)
                    }
                }
                Button(
                    onClick = concluir,
                    contentPadding = PaddingValues(horizontal = 12.dp, vertical = 2.dp),
                ) {
                    Text("Concluir", fontSize = 12.sp)
                }
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedButton(onClick = { renomeando = true }) { Text("✏️ Renomear") }
            OutlinedButton(onClick = { criandoPainel = true }) { Text("+ Novo painel") }
            if (aparelho.paineis.size > 1) {
                OutlinedButton(onClick = { confirmandoExclusao = true }) { Text("🗑️ Excluir") }
            }
            if (aoInverterLados != null) {
                OutlinedButton(onClick = aoInverterLados) { Text("⇄ Inverter lados") }
            }
            OutlinedButton(onClick = { escolhendo = true }) { Text("Adicionar card") }
            OutlinedButton(onClick = { processo = true }) { Text("Acompanhar processo") }
            TextButton(onClick = { aparelho.editarArranjo { Arranjo.PADRAO.conhecer(it.conhecidas) } }) {
                Text("Restaurar padrão")
            }
        }
    }

    if (renomeando) {
        DialogoRenomearPainel(
            nomeAtual = painelAtual.nome,
            aoConfirmar = { novoNome ->
                aparelho.aoRenomearPainel(painelAtual.id, novoNome)
                renomeando = false
            },
            aoFechar = { renomeando = false },
        )
    }

    if (criandoPainel) {
        DialogoCriarPainel(
            numeroSugerido = aparelho.paineis.size + 1,
            aoConfirmar = { nome ->
                aparelho.aoCriarPainel(nome)
                criandoPainel = false
            },
            aoFechar = { criandoPainel = false },
        )
    }

    if (confirmandoExclusao) {
        AlertDialog(
            onDismissRequest = { confirmandoExclusao = false },
            title = { Text("Excluir painel") },
            text = { Text("Tem certeza que deseja excluir o painel \"${painelAtual.nome}\"? Esta ação não pode ser desfeita.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        aparelho.aoExcluirPainel(painelAtual.id)
                        confirmandoExclusao = false
                    },
                ) { Text("Excluir", color = Cores.ruim) }
            },
            dismissButton = {
                TextButton(onClick = { confirmandoExclusao = false }) { Text("Cancelar") }
            },
        )
    }

    if (escolhendo) {
        EscolherCard(aparelho, rotulos) { escolhendo = false }
    }
    if (processo) {
        AcompanharProcesso(aparelho, sugestoes = emptyList()) { processo = false }
    }
}

@Composable
private fun DialogoRenomearPainel(
    nomeAtual: String,
    aoConfirmar: (String) -> Unit,
    aoFechar: () -> Unit,
) {
    var nome by remember { mutableStateOf(nomeAtual) }
    AlertDialog(
        onDismissRequest = aoFechar,
        title = { Text("Renomear painel") },
        text = {
            OutlinedTextField(
                value = nome,
                onValueChange = { nome = it.take(24) },
                label = { Text("Nome do painel") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            Button(
                onClick = { if (nome.isNotBlank()) aoConfirmar(nome) },
                enabled = nome.isNotBlank(),
            ) { Text("Salvar") }
        },
        dismissButton = {
            TextButton(onClick = aoFechar) { Text("Cancelar") }
        },
    )
}

@Composable
private fun DialogoCriarPainel(
    numeroSugerido: Int,
    aoConfirmar: (String) -> Unit,
    aoFechar: () -> Unit,
) {
    var nome by remember { mutableStateOf("Painel $numeroSugerido") }
    AlertDialog(
        onDismissRequest = aoFechar,
        title = { Text("Novo painel") },
        text = {
            OutlinedTextField(
                value = nome,
                onValueChange = { nome = it.take(24) },
                label = { Text("Nome do painel") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            Button(
                onClick = { if (nome.isNotBlank()) aoConfirmar(nome) },
                enabled = nome.isNotBlank(),
            ) { Text("Criar") }
        },
        dismissButton = {
            TextButton(onClick = aoFechar) { Text("Cancelar") }
        },
    )
}

@Composable
private fun EscolherCard(aparelho: Aparelho, rotulos: Map<String, String>, fechar: () -> Unit) {
    val (sensores, cards) = aparelho.arranjo.disponiveis(rotulos).partition(Arranjo::ehSensor)
    val opcao: @Composable (String) -> Unit = { chave ->
        Text(
            rotulos.getValue(chave),
            fontSize = 17.sp,
            modifier = Modifier
                .fillMaxWidth()
                .clickable {
                    aparelho.editarArranjo { it.adicionar(chave) }
                    fechar()
                }
                .padding(vertical = 12.dp),
        )
    }
    AlertDialog(
        onDismissRequest = fechar,
        confirmButton = { TextButton(onClick = fechar) { Text("Fechar") } },
        title = { Text("Adicionar card") },
        text = {
            // Com um sensor por núcleo, a lista passa da tela: rola por dentro do diálogo.
            Column(
                Modifier.heightIn(max = 420.dp).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (cards.isEmpty() && sensores.isEmpty()) Text("Todos os cards desta tela já estão à vista.")
                cards.forEach { opcao(it) }
                if (sensores.isNotEmpty()) {
                    Text(
                        "Sensores de temperatura",
                        color = Cores.textoFraco,
                        fontSize = 13.sp,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                    sensores.forEach { opcao(it) }
                }
            }
        },
    )
}
