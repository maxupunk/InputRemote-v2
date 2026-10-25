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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.ui.Tela
import io.github.inputremote.painel.ui.tema.Cores

/**
 * Tela de ajustes rápidos e preferências do painel no tablet.
 */
@Composable
fun TelaDeAjustes(
    painel: Tela.Painel,
    acoes: Acoes,
    aparelho: Aparelho,
    aoIniciarEdicao: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        // Visor e Aparelho
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(containerColor = Cores.superficie),
            border = BorderStroke(1.dp, Cores.borda),
        ) {
            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(
                    "Visor e Aparelho",
                    fontSize = 15.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = Cores.texto,
                )

                // Escurecer tela
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text("Escurecer a tela", color = Cores.texto, fontSize = 14.sp)
                        Text(
                            "Reduz o brilho para 5%, ideal para noite e proteção do painel",
                            color = Cores.textoFraco,
                            fontSize = 12.sp,
                        )
                    }
                    Switch(
                        checked = aparelho.escurecido,
                        onCheckedChange = { aparelho.alternarEscurecer() },
                        colors = SwitchDefaults.colors(
                            checkedThumbColor = Cores.acao,
                            checkedTrackColor = Cores.borda,
                        ),
                    )
                }

                // Sempre deitado
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text("Sempre deitado (paisagem)", color = Cores.texto, fontSize = 14.sp)
                        Text(
                            "Mantém a tela deitada para suporte de mesa",
                            color = Cores.textoFraco,
                            fontSize = 12.sp,
                        )
                    }
                    Switch(
                        checked = aparelho.sempreDeitado,
                        onCheckedChange = { aparelho.alternarDeitado() },
                        colors = SwitchDefaults.colors(
                            checkedThumbColor = Cores.acao,
                            checkedTrackColor = Cores.borda,
                        ),
                    )
                }

                // Personalizar cards
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(8.dp))
                        .clickable(onClick = aoIniciarEdicao)
                        .background(Cores.fundo)
                        .padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text("Personalizar cards do painel", color = Cores.texto, fontSize = 14.sp, fontWeight = FontWeight.Medium)
                        Text("Reorganizar ordem, alternar destaque ou ocultar métricas", color = Cores.textoFraco, fontSize = 12.sp)
                    }
                    Text("Editar ➔", color = Cores.acao, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }

        // Conexão e Pareamento
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(containerColor = Cores.superficie),
            border = BorderStroke(1.dp, Cores.borda),
        ) {
            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(
                    "Conexão e Pareamento",
                    fontSize = 15.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = Cores.texto,
                )

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Ponto(if (painel.segundosSemDados > 0) Cores.atencao else Cores.bom)
                    Spacer(Modifier.width(8.dp))
                    Text(
                        "Conectado a: ${painel.computador}",
                        color = Cores.texto,
                        fontSize = 14.sp,
                        fontWeight = FontWeight.Medium,
                    )
                }

                if (painel.segundosSemDados > 0) {
                    Text(
                        "Aviso: sem dados atualizados há ${painel.segundosSemDados} segundos",
                        color = Cores.atencao,
                        fontSize = 12.sp,
                    )
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    OutlinedButton(
                        onClick = acoes.procurar,
                        modifier = Modifier.weight(1f),
                    ) {
                        Text("Parear outro PC")
                    }
                    Button(
                        onClick = acoes.esquecer,
                        colors = ButtonDefaults.buttonColors(containerColor = Cores.superficie),
                        border = BorderStroke(1.dp, Cores.ruim),
                        modifier = Modifier.weight(1f),
                    ) {
                        Text("Esquecer este PC", color = Cores.ruim)
                    }
                }
            }
        }

        // Sobre
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(containerColor = Cores.superficie),
            border = BorderStroke(1.dp, Cores.borda),
        ) {
            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("InputRemote Painel v0.1.0", fontSize = 14.sp, fontWeight = FontWeight.SemiBold, color = Cores.texto)
                Text(
                    "Telemetria em tempo real, sem serviços pesados em segundo plano.",
                    fontSize = 12.sp,
                    color = Cores.textoFraco,
                )
            }
        }
    }
}
