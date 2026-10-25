package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.foundation.rememberScrollState
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
 * Um card em edição: o card, com a borda de ação, e os controles embaixo — andar para antes ou para
 * depois, destacar, tirar. Fora da edição, só o card.
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
    val destaque = aparelho.arranjo.cards.firstOrNull { it.chave == chave }?.destaque == true
    val editar = aparelho.editarArranjo
    Column(
        modifier.border(BorderStroke(1.dp, Cores.acao), RoundedCornerShape(12.dp)),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        conteudo()
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp), horizontalArrangement = Arrangement.SpaceEvenly) {
            if (podeMover) {
                Controle("◀", "Mover para antes") { editar { it.mover(chave, -1) } }
                Controle("▶", "Mover para depois") { editar { it.mover(chave, 1) } }
            }
            if (podeDestacar) {
                Controle(if (destaque) "★" else "☆", if (destaque) "Deixar compacto" else "Destacar") {
                    editar { it.alternarDestaque(chave) }
                }
            }
            Controle("✕", "Tirar da tela", Cores.ruim) { editar { it.remover(chave) } }
        }
    }
}

/** Um botão de edição: um símbolo grande para o dedo, e o nome para o leitor de tela. */
@Composable
private fun Controle(simbolo: String, nome: String, cor: androidx.compose.ui.graphics.Color = Cores.acao, acao: () -> Unit) {
    Box(
        Modifier
            .size(44.dp)
            .clickable(onClick = acao)
            .semantics { contentDescription = nome },
        contentAlignment = Alignment.Center,
    ) {
        Text(simbolo, color = cor, fontSize = 20.sp)
    }
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
                text = "Editando \"${painelAtual.nome}\": toque ◀ ▶ na ordem, ★ no destaque, ✕ para tirar.",
                color = Cores.textoFraco,
                fontSize = 12.sp,
                modifier = Modifier.weight(1f, fill = false),
            )
            Button(
                onClick = concluir,
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 2.dp),
            ) {
                Text("Concluir", fontSize = 12.sp)
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
            Button(onClick = concluir) { Text("Concluir") }
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
    val disponiveis = aparelho.arranjo.ocultas().filter { it in rotulos }.sortedBy { rotulos[it] }
    AlertDialog(
        onDismissRequest = fechar,
        confirmButton = { TextButton(onClick = fechar) { Text("Fechar") } },
        title = { Text("Adicionar card") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                if (disponiveis.isEmpty()) Text("Todos os cards desta tela já estão à vista.")
                disponiveis.forEach { chave ->
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
            }
        },
    )
}
