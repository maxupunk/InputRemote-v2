package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
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

@Composable
private fun CardCopiaCompacto(
    copia: Bloco?,
    series: Map<String, List<Float>>,
    modifier: Modifier = Modifier,
) {
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, Cores.borda),
    ) {
        Column(
            Modifier.padding(horizontal = 10.dp, vertical = 8.dp).fillMaxHeight(),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                "Transferência de Arquivos",
                fontSize = 14.sp,
                fontWeight = FontWeight.SemiBold,
                color = Cores.texto,
            )
            if (copia != null) {
                val cor = copia.nivel.cor()
                val serie = series[Historico.chave(Historico.ENLACE, copia.chave)].orEmpty()
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f)
                        .clip(RoundedCornerShape(10.dp))
                        .background(Cores.fundo)
                        .padding(horizontal = 10.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.SpaceBetween,
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(copia.rotulo, color = Cores.textoFraco, fontSize = 12.sp)
                        Text(copia.valor, style = numeros, fontSize = 22.sp, color = Cores.bom)
                    }
                    copia.detalhe?.let {
                        Text(
                            it,
                            color = Cores.texto,
                            fontSize = 12.sp,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    copia.fracao?.let { Barra(it, Cores.bom, 5.dp) }
                    if (serie.size > 1) {
                        MiniGrafico(serie, cor, Modifier.fillMaxWidth().height(18.dp))
                    }
                }
            } else {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f)
                        .clip(RoundedCornerShape(10.dp))
                        .background(Cores.fundo)
                        .padding(10.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text(
                            "Nenhuma cópia no momento",
                            color = Cores.textoFraco,
                            fontSize = 13.sp,
                            fontWeight = FontWeight.Medium,
                        )
                        Text(
                            "Ao transferir arquivos entre os computadores, o progresso e velocidade aparecerão aqui.",
                            color = Cores.textoFraco,
                            fontSize = 11.sp,
                            textAlign = TextAlign.Center,
                            lineHeight = 15.sp,
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
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, Cores.borda),
    ) {
        Column(
            Modifier.padding(horizontal = 10.dp, vertical = 8.dp).fillMaxHeight(),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                "Tráfego de Rede por Máquina",
                fontSize = 14.sp,
                fontWeight = FontWeight.SemiBold,
                color = Cores.texto,
            )
            Column(
                modifier = Modifier.fillMaxWidth().weight(1f),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                vista.maquinas.forEach { maquina ->
                    val rede = maquina.blocos.firstOrNull { it.chave == "rede" }
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .weight(1f)
                            .clip(RoundedCornerShape(10.dp))
                            .background(Cores.fundo)
                            .padding(horizontal = 10.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            Text(
                                maquina.nome,
                                color = Cores.texto,
                                fontSize = 13.sp,
                                fontWeight = FontWeight.SemiBold,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            rede?.detalhe?.let {
                                Text(
                                    it,
                                    color = Cores.textoFraco,
                                    fontSize = 11.sp,
                                    style = numeros,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                        Text(
                            rede?.valor ?: "—",
                            style = numeros,
                            fontSize = 18.sp,
                            color = Cores.texto,
                            maxLines = 1,
                        )
                    }
                }
            }
        }
    }
}
