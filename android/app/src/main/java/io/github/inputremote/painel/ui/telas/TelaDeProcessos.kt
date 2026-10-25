package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.dados.ListaEstavel
import io.github.inputremote.painel.dados.Ordem
import io.github.inputremote.painel.dados.VistaDaMaquina
import io.github.inputremote.painel.ui.tema.Cores

/**
 * Tela completa dedicada ao monitoramento ao vivo de processos dos computadores.
 * Permite filtrar, pesquisar, ordenar por CPU/Memória e acompanhar processos no painel.
 */
@Composable
fun TelaDeProcessos(
    maquinas: List<VistaDaMaquina>,
    maquinaIndex: Int,
    aoMudarMaquina: (Int) -> Unit,
    ordemInicial: Ordem,
    aparelho: Aparelho,
    modifier: Modifier = Modifier,
) {
    var ordem by rememberSaveable(ordemInicial) { mutableStateOf(ordemInicial) }
    var filtroTexto by rememberSaveable { mutableStateOf("") }
    var outro by remember { mutableStateOf(false) }
    val estavel = remember { ListaEstavel() }

    val maquina = maquinas.getOrNull(maquinaIndex) ?: maquinas.firstOrNull()
    if (maquina == null) {
        Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text("Nenhum computador conectado no momento.", color = Cores.textoFraco, fontSize = 15.sp)
        }
        return
    }

    Card(
        modifier = modifier.fillMaxSize(),
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, Cores.borda),
    ) {
        Column(
            modifier = Modifier.padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            // Cabeçalho e seletor de máquina
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        "Processos em Tempo Real",
                        fontSize = 16.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = Cores.texto,
                    )
                    Text(
                        "Atualiza a cada segundo com consumo de CPU e RAM.",
                        fontSize = 12.sp,
                        color = Cores.textoFraco,
                    )
                }

                if (maquinas.size > 1) {
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        maquinas.forEachIndexed { indice, m ->
                            FilterChip(
                                selected = maquinaIndex == indice,
                                onClick = { aoMudarMaquina(indice) },
                                label = { Text(m.nome) },
                            )
                        }
                    }
                }
            }

            // Barra de controles: Ordenação, Filtro e Atalhos
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Ordenar:", fontSize = 13.sp, color = Cores.textoFraco)
                Ordem.entries.forEach { opcao ->
                    FilterChip(
                        selected = ordem == opcao,
                        onClick = { ordem = opcao },
                        label = { Text(opcao.nome) },
                    )
                }

                OutlinedTextField(
                    value = filtroTexto,
                    onValueChange = { filtroTexto = it },
                    placeholder = { Text("Filtrar por nome…", fontSize = 13.sp) },
                    singleLine = true,
                    modifier = Modifier.weight(1f),
                )

                OutlinedButton(onClick = { outro = true }) {
                    Text("+ Acompanhar outro…", fontSize = 13.sp)
                }
            }

            // Atalhos rápidos populares
            Row(
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Atalhos:", fontSize = 12.sp, color = Cores.textoFraco)
                listOf("docker", "wsl", "vmmem", "chrome", "code").forEach { atalho ->
                    SuggestionChip(
                        onClick = { filtroTexto = atalho },
                        label = { Text(atalho, fontSize = 12.sp) },
                    )
                }
                if (filtroTexto.isNotEmpty()) {
                    SuggestionChip(
                        onClick = { filtroTexto = "" },
                        label = { Text("✕ Limpar", fontSize = 12.sp, color = Cores.acao) },
                    )
                }
            }

            val linhasBrutas = maquina.processos
            if (linhasBrutas == null) {
                Medindo()
            } else {
                val linhasFiltradas = if (filtroTexto.isBlank()) {
                    linhasBrutas
                } else {
                    linhasBrutas.filter { it.nome.contains(filtroTexto, ignoreCase = true) }
                }

                val ordenadas = estavel.arrumar(linhasFiltradas, ordem, System.currentTimeMillis())
                if (ordenadas.isEmpty()) {
                    Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) {
                        Text(
                            if (filtroTexto.isNotBlank()) "Nenhum processo com \"$filtroTexto\"." else "Nenhum processo para mostrar.",
                            color = Cores.textoFraco,
                            fontSize = 14.sp,
                        )
                    }
                } else {
                    LazyColumn(
                        modifier = Modifier.weight(1f),
                        verticalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        items(ordenadas, key = { it.chave }) { linha ->
                            Linha(linha, ordem, ordem.fracao(linha, ordenadas), aparelho)
                        }
                    }
                }
            }
        }
    }

    if (outro) {
        AcompanharProcesso(aparelho, maquina.processos.orEmpty().map { it.nome }) { outro = false }
    }
}
