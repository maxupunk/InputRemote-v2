package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.dados.Historico
import io.github.inputremote.painel.dados.Vista
import io.github.inputremote.painel.dados.VistaDoEnlace
import io.github.inputremote.painel.dados.Bloco
import io.github.inputremote.painel.ui.tema.Cores
import io.github.inputremote.painel.ui.tema.cor
import io.github.inputremote.painel.ui.tema.numeros

/**
 * Tela dedicada ao monitoramento de conexões, enlaces e transferências.
 * Mostra gráficos de latência Bluetooth e Wi-Fi e detalhes de cópia de arquivos.
 * Projetada de forma compacta e ergonômica para caber inteiramente na tela sem necessidade de rolagem.
 */
@Composable
fun TelaDeConexoes(
    vista: Vista,
    series: Map<String, List<Float>>,
    modifier: Modifier = Modifier,
) {
    BoxWithConstraints(modifier.fillMaxSize()) {
        val deitado = maxWidth > maxHeight

        if (deitado) {
            // Em orientação deitada: Enlace no topo e Transferências + Tráfego lado a lado em 2 colunas
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(horizontal = 2.dp, vertical = 2.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                val enlace = vista.enlace
                if (enlace != null) {
                    CardEnlaceCompacto(enlace = enlace, series = series)
                }

                Row(
                    modifier = Modifier.fillMaxWidth().weight(1f),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    CardCopiaCompacto(
                        copia = vista.copia,
                        series = series,
                        modifier = Modifier.weight(1f).fillMaxHeight(),
                    )
                    CardTrafegoRedeCompacto(
                        vista = vista,
                        series = series,
                        modifier = Modifier.weight(1f).fillMaxHeight(),
                    )
                }
            }
        } else {
            // Em orientação em pé: empilhado com rolagem suave se a tela for estreita
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(2.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                val enlace = vista.enlace
                if (enlace != null) {
                    CardEnlaceCompacto(enlace = enlace, series = series)
                }
                CardCopiaCompacto(copia = vista.copia, series = series, modifier = Modifier.fillMaxWidth())
                CardTrafegoRedeCompacto(vista = vista, series = series, modifier = Modifier.fillMaxWidth())
            }
        }
    }
}

@Composable
private fun CardEnlaceCompacto(
    enlace: VistaDoEnlace,
    series: Map<String, List<Float>>,
    modifier: Modifier = Modifier,
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, Cores.borda),
    ) {
        Column(
            Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "Enlace entre os Computadores",
                    fontSize = 14.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = Cores.texto,
                    modifier = Modifier.weight(1f),
                )
                enlace.vencendo?.let {
                    Row(
                        modifier = Modifier
                            .clip(RoundedCornerShape(8.dp))
                            .background(Cores.fundo)
                            .padding(horizontal = 8.dp, vertical = 3.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text(
                            "Mais rápida: ${it.replaceFirstChar { c -> c.uppercase() }}",
                            color = Cores.bom,
                            fontSize = 11.sp,
                            fontWeight = FontWeight.Medium,
                        )
                    }
                }
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                enlace.rotas.forEach { bloco ->
                    val serie = series[Historico.chave(Historico.ENLACE, bloco.chave)].orEmpty()
                    val cor = bloco.nivel.cor()
                    Column(
                        modifier = Modifier
                            .weight(1f)
                            .clip(RoundedCornerShape(10.dp))
                            .background(Cores.fundo)
                            .padding(horizontal = 8.dp, vertical = 5.dp),
                        verticalArrangement = Arrangement.spacedBy(1.dp),
                    ) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(bloco.rotulo, color = Cores.textoFraco, fontSize = 11.sp, modifier = Modifier.weight(1f))
                            if (bloco.chave == enlace.vencendo) {
                                Text("ativa", color = Cores.bom, fontSize = 10.sp, fontWeight = FontWeight.SemiBold)
                            }
                        }
                        Text(
                            bloco.valor,
                            style = numeros,
                            fontSize = 20.sp,
                            color = Cores.texto,
                        )
                        if (serie.size > 1) {
                            MiniGrafico(serie, cor, Modifier.fillMaxWidth().height(18.dp))
                        } else {
                            Spacer(Modifier.height(18.dp))
                        }
                    }
                }
            }
        }
    }
}

private data class DetalhesCopia(
    val taxa: String?,
    val tempoRestante: String?,
    val nomeArquivo: String,
)

private fun extrairDetalhesCopia(detalhe: String?): DetalhesCopia? {
    if (detalhe.isNullOrBlank()) return null
    val partes = detalhe.split("·").map { it.trim() }
    return if (partes.size >= 3) {
        DetalhesCopia(
            taxa = partes[0],
            tempoRestante = partes[1],
            nomeArquivo = partes.drop(2).joinToString("·"),
        )
    } else if (partes.size == 2) {
        DetalhesCopia(
            taxa = partes[0],
            tempoRestante = null,
            nomeArquivo = partes[1],
        )
    } else {
        DetalhesCopia(
            taxa = null,
            tempoRestante = null,
            nomeArquivo = detalhe,
        )
    }
}

private data class RegistroDeCopia(
    val nome: String,
    val direcao: String,
    val horario: String,
    val taxa: String? = null,
)

private enum class ModoTrafegoRede {
    GRAFICO,
    NUMERICO,
}

@Composable
private fun CardCopiaCompacto(
    copia: Bloco?,
    series: Map<String, List<Float>>,
    modifier: Modifier = Modifier,
) {
    var ultimoRegistro by remember { mutableStateOf<RegistroDeCopia?>(null) }

    LaunchedEffect(copia?.detalhe, copia?.valor) {
        if (copia != null) {
            val info = extrairDetalhesCopia(copia.detalhe)
            val nome = info?.nomeArquivo ?: "Arquivo"
            val direcao = if (copia.rotulo.contains("enviando", ignoreCase = true)) "Enviado" else "Recebido"
            val agora = java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault()).format(java.util.Date())
            ultimoRegistro = RegistroDeCopia(
                nome = nome,
                direcao = direcao,
                horario = agora,
                taxa = info?.taxa,
            )
        }
    }

    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, Cores.borda),
    ) {
        Column(
            Modifier.padding(horizontal = 10.dp, vertical = 8.dp).fillMaxHeight(),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "Transferência de Arquivos",
                    fontSize = 14.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = Cores.texto,
                )
                if (copia != null) {
                    val direcao = if (copia.rotulo.contains("enviando", ignoreCase = true)) "Enviando" else "Recebendo"
                    Text(
                        direcao,
                        color = Cores.bom,
                        fontSize = 11.sp,
                        fontWeight = FontWeight.Medium,
                        modifier = Modifier
                            .clip(RoundedCornerShape(6.dp))
                            .background(Cores.fundo)
                            .padding(horizontal = 6.dp, vertical = 2.dp),
                    )
                } else if (ultimoRegistro != null) {
                    Text(
                        "Concluído",
                        color = Cores.textoFraco,
                        fontSize = 11.sp,
                        modifier = Modifier
                            .clip(RoundedCornerShape(6.dp))
                            .background(Cores.fundo)
                            .padding(horizontal = 6.dp, vertical = 2.dp),
                    )
                }
            }

            if (copia != null) {
                val cor = copia.nivel.cor()
                val serie = series[Historico.chave(Historico.ENLACE, copia.chave)].orEmpty()
                val info = extrairDetalhesCopia(copia.detalhe)

                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f)
                        .clip(RoundedCornerShape(10.dp))
                        .background(Cores.fundo)
                        .padding(horizontal = 10.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    // Nome do arquivo e porcentagem
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            info?.nomeArquivo ?: copia.rotulo,
                            color = Cores.texto,
                            fontSize = 14.sp,
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f).padding(end = 8.dp),
                        )
                        Text(
                            copia.valor,
                            style = numeros,
                            fontSize = 20.sp,
                            color = Cores.bom,
                        )
                    }

                    // Barra de progresso com altura padronizada
                    copia.fracao?.let { Barra(it, Cores.bom, 6.dp) }

                    // Métricas: Taxa e tempo restante lado a lado
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        info?.taxa?.let {
                            Text(
                                "⚡ $it",
                                color = Cores.texto,
                                fontSize = 12.sp,
                                style = numeros,
                            )
                        }
                        info?.tempoRestante?.let {
                            Text(
                                "⏱ $it",
                                color = Cores.textoFraco,
                                fontSize = 12.sp,
                            )
                        }
                    }

                    // Gráfico da taxa de cópia, se houver histórico
                    if (serie.size > 1) {
                        MiniGrafico(serie, cor, Modifier.fillMaxWidth().height(26.dp))
                    } else {
                        Spacer(Modifier.height(4.dp))
                    }
                }

                // Quando houver espaço no card e histórico anterior
                ultimoRegistro?.let { hist ->
                    if (hist.nome != (info?.nomeArquivo ?: "")) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(8.dp))
                                .background(Cores.fundo.copy(alpha = 0.6f))
                                .padding(horizontal = 8.dp, vertical = 4.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            Text("✓", color = Cores.bom, fontSize = 11.sp, fontWeight = FontWeight.Bold)
                            Text(
                                "Anterior: ${hist.nome} (${hist.direcao} às ${hist.horario})",
                                color = Cores.textoFraco,
                                fontSize = 11.sp,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
            } else {
                // Estado sem cópia ativa no momento
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    val hist = ultimoRegistro
                    if (hist != null) {
                        // Exibe card com a última transferência concluída (histórico preservado)
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(10.dp))
                                .background(Cores.fundo)
                                .padding(horizontal = 10.dp, vertical = 8.dp),
                            verticalArrangement = Arrangement.spacedBy(4.dp),
                        ) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                                ) {
                                    Text("✓", color = Cores.bom, fontSize = 13.sp, fontWeight = FontWeight.Bold)
                                    Text(
                                        "Última Transferência",
                                        color = Cores.textoFraco,
                                        fontSize = 11.sp,
                                    )
                                }
                                Text(
                                    hist.horario,
                                    color = Cores.textoFraco,
                                    fontSize = 11.sp,
                                    style = numeros,
                                )
                            }
                            Text(
                                hist.nome,
                                color = Cores.texto,
                                fontSize = 14.sp,
                                fontWeight = FontWeight.SemiBold,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    "${hist.direcao} com sucesso",
                                    color = Cores.bom,
                                    fontSize = 11.sp,
                                )
                                hist.taxa?.let {
                                    Text("· $it", color = Cores.textoFraco, fontSize = 11.sp, style = numeros)
                                }
                            }
                        }
                    }

                    // Aviso explicativo discreto
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .weight(1f)
                            .clip(RoundedCornerShape(10.dp))
                            .background(Cores.fundo.copy(alpha = 0.5f))
                            .padding(8.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            if (hist != null) "Aguardando próxima transferência…" else "Nenhuma cópia no momento. Ao transferir arquivos entre os computadores, os detalhes aparecerão aqui.",
                            color = Cores.textoFraco,
                            fontSize = 12.sp,
                            textAlign = TextAlign.Center,
                            lineHeight = 16.sp,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun CardTrafegoRedeCompacto(
    vista: Vista,
    series: Map<String, List<Float>>,
    modifier: Modifier = Modifier,
) {
    var modo by remember { mutableStateOf(ModoTrafegoRede.GRAFICO) }

    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, Cores.borda),
    ) {
        Column(
            Modifier.padding(horizontal = 10.dp, vertical = 8.dp).fillMaxHeight(),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "Tráfego de Rede",
                    fontSize = 14.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = Cores.texto,
                )
                // Seletor de modo elegante (Gráfico vs Numérico)
                Row(
                    modifier = Modifier
                        .clip(RoundedCornerShape(8.dp))
                        .background(Cores.fundo)
                        .padding(2.dp),
                    horizontalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    val selecionadoGrafico = modo == ModoTrafegoRede.GRAFICO
                    Text(
                        "Gráfico",
                        fontSize = 11.sp,
                        fontWeight = if (selecionadoGrafico) FontWeight.SemiBold else FontWeight.Normal,
                        color = if (selecionadoGrafico) Cores.texto else Cores.textoFraco,
                        modifier = Modifier
                            .clip(RoundedCornerShape(6.dp))
                            .background(if (selecionadoGrafico) Cores.superficie else Color.Transparent)
                            .clickable { modo = ModoTrafegoRede.GRAFICO }
                            .padding(horizontal = 8.dp, vertical = 3.dp),
                    )
                    val selecionadoNumerico = modo == ModoTrafegoRede.NUMERICO
                    Text(
                        "Numérico",
                        fontSize = 11.sp,
                        fontWeight = if (selecionadoNumerico) FontWeight.SemiBold else FontWeight.Normal,
                        color = if (selecionadoNumerico) Cores.texto else Cores.textoFraco,
                        modifier = Modifier
                            .clip(RoundedCornerShape(6.dp))
                            .background(if (selecionadoNumerico) Cores.superficie else Color.Transparent)
                            .clickable { modo = ModoTrafegoRede.NUMERICO }
                            .padding(horizontal = 8.dp, vertical = 3.dp),
                    )
                }
            }

            Column(
                modifier = Modifier.fillMaxWidth().weight(1f),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                vista.maquinas.forEach { maquina ->
                    val rede = maquina.blocos.firstOrNull { it.chave == "rede" }
                    val serie = series[Historico.chave(maquina.nome, "rede")].orEmpty()

                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .weight(1f)
                            .clip(RoundedCornerShape(10.dp))
                            .background(Cores.fundo)
                            .padding(horizontal = 10.dp, vertical = 6.dp),
                        verticalArrangement = Arrangement.SpaceBetween,
                    ) {
                        // Cabeçalho da máquina com nome e valores atuais
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                maquina.nome,
                                color = Cores.texto,
                                fontSize = 13.sp,
                                fontWeight = FontWeight.SemiBold,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    rede?.valor ?: "—",
                                    style = numeros,
                                    fontSize = 13.sp,
                                    color = Cores.bom,
                                    maxLines = 1,
                                )
                                rede?.detalhe?.let {
                                    Text(
                                        it,
                                        style = numeros,
                                        fontSize = 12.sp,
                                        color = Cores.textoFraco,
                                        maxLines = 1,
                                    )
                                }
                            }
                        }

                        if (modo == ModoTrafegoRede.GRAFICO) {
                            // Modo Gráfico: Sparkline padronizado de 28dp preenchendo o espaço uniformemente
                            if (serie.size > 1) {
                                MiniGrafico(serie, Cores.bom, Modifier.fillMaxWidth().height(28.dp))
                            } else {
                                Box(
                                    Modifier
                                        .fillMaxWidth()
                                        .height(28.dp)
                                        .background(Cores.borda.copy(alpha = 0.2f), RoundedCornerShape(4.dp)),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Text("aguardando dados de rede…", fontSize = 10.sp, color = Cores.textoFraco)
                                }
                            }
                        } else {
                            // Modo Numérico: Cards dedicados lado a lado para Download e Upload
                            Row(
                                modifier = Modifier.fillMaxWidth().height(36.dp),
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Column(
                                    modifier = Modifier
                                        .weight(1f)
                                        .fillMaxHeight()
                                        .clip(RoundedCornerShape(6.dp))
                                        .background(Cores.superficie)
                                        .padding(horizontal = 8.dp, vertical = 2.dp),
                                    verticalArrangement = Arrangement.Center,
                                ) {
                                    Text("Entrada (Download)", fontSize = 9.sp, color = Cores.textoFraco)
                                    Text(
                                        rede?.valor?.removePrefix("↓ ") ?: "—",
                                        style = numeros,
                                        fontSize = 14.sp,
                                        color = Cores.bom,
                                        fontWeight = FontWeight.Bold,
                                    )
                                }
                                Column(
                                    modifier = Modifier
                                        .weight(1f)
                                        .fillMaxHeight()
                                        .clip(RoundedCornerShape(6.dp))
                                        .background(Cores.superficie)
                                        .padding(horizontal = 8.dp, vertical = 2.dp),
                                    verticalArrangement = Arrangement.Center,
                                ) {
                                    Text("Saída (Upload)", fontSize = 9.sp, color = Cores.textoFraco)
                                    Text(
                                        rede?.detalhe?.removePrefix("↑ ") ?: "—",
                                        style = numeros,
                                        fontSize = 14.sp,
                                        color = Cores.texto,
                                        fontWeight = FontWeight.Bold,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
