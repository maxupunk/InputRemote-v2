package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.dados.ComputadorAchado
import io.github.inputremote.painel.ui.Tela
import io.github.inputremote.painel.ui.tema.Cores

/** Uma coluna centrada e de largura de leitura, para as telas de texto. */
@Composable
private fun Centro(conteudo: @Composable () -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Column(Modifier.widthIn(max = 560.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            conteudo()
        }
    }
}

@Composable
private fun Titulo(texto: String) = Text(texto, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, color = Cores.texto)

@Composable
private fun Nota(texto: String, cor: androidx.compose.ui.graphics.Color = Cores.textoFraco) =
    Text(texto, fontSize = 15.sp, color = cor)

/** Sem computador pareado: procurar um na rede. */
@Composable
fun TelaDeProcura(procura: Tela.Procurando, acoes: Acoes) = Centro {
    Titulo("Parear com um computador")
    procura.recado?.let { Nota(it, Cores.atencao) }
    Nota(
        "No computador, abra o InputRemote, toque em \"Painel no celular\" e depois em " +
            "\"Parear aparelho\". Este aparelho precisa estar no mesmo Wi-Fi.",
    )
    if (procura.procurando) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            Nota("Procurando computadores na rede…")
        }
    } else if (procura.achados.isEmpty()) {
        Nota(
            "Nenhum computador respondeu. Confira se o InputRemote está aberto no computador e se " +
                "os dois estão na mesma rede — ou digite o endereço dele abaixo.",
        )
    }
    procura.achados.forEach { CartaoDeAchado(it) { acoes.parear(it) } }
    OutlinedButton(onClick = acoes.procurar, enabled = !procura.procurando) { Text("Procurar de novo") }
    EnderecoDigitado(acoes)
}

@Composable
private fun CartaoDeAchado(computador: ComputadorAchado, escolher: () -> Unit) {
    Card(
        modifier = Modifier.fillMaxWidth().clickable(onClick = escolher),
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, Cores.borda),
    ) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(computador.nome, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = Cores.texto)
                Text(computador.endereco, fontSize = 13.sp, color = Cores.textoFraco)
            }
            Text("Parear", color = Cores.acao, fontWeight = FontWeight.SemiBold)
        }
    }
}

/** Para a rede em que a procura não alcança: o IP do computador, digitado. */
@Composable
private fun EnderecoDigitado(acoes: Acoes) {
    var endereco by rememberSaveable { mutableStateOf("") }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedTextField(
            value = endereco,
            onValueChange = { endereco = it.trim() },
            label = { Text("IP do computador") },
            placeholder = { Text("192.168.0.10") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            modifier = Modifier.weight(1f),
        )
        TextButton(onClick = { acoes.parear(ComputadorAchado(endereco, endereco)) }, enabled = endereco.isNotEmpty()) {
            Text("Parear")
        }
    }
}

/** O código nas duas telas. */
@Composable
fun TelaDeConferencia(conferindo: Tela.Conferindo, acoes: Acoes) = Centro {
    Titulo("Parear com ${conferindo.computador}")
    val digitos = conferindo.digitos
    when {
        digitos == null -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            Nota("Esperando o computador… Confira se \"Parear aparelho\" está aberto na janela dele.")
        }
        conferindo.aguardandoOutraTela -> {
            Digitos(digitos)
            Nota("Agora confirme também na janela do computador.")
        }
        else -> {
            Nota("Estes seis dígitos aparecem iguais na janela do computador?")
            Digitos(digitos)
            Nota("Se forem diferentes, alguém pode estar no meio da conexão — e aí não é para parear.")
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Button(onClick = { acoes.confirmar(true) }) { Text("São iguais") }
                OutlinedButton(onClick = { acoes.confirmar(false) }) { Text("São diferentes", color = Cores.ruim) }
            }
        }
    }
    TextButton(onClick = acoes.procurar) { Text("Cancelar") }
}

/** Ligando, ou religando depois de uma queda. */
@Composable
fun TelaDeConexao(conectando: Tela.Conectando, acoes: Acoes) = Centro {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
        Titulo("Conectando a ${conectando.computador}…")
    }
    conectando.motivo?.let { Nota("Da última vez: $it. Tentando de novo sozinho.") }
    Nota("O computador precisa estar ligado e na mesma rede. A tela volta sozinha quando ele responder.")
    TextButton(onClick = acoes.procurar) { Text("Parear outro computador") }
}

/** Abrindo, por um instante. */
@Composable
fun TelaDeAbertura() = Centro {
    CircularProgressIndicator()
}
