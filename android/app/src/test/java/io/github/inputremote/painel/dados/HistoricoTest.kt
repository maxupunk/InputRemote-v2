package io.github.inputremote.painel.dados

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HistoricoTest {
    private fun vista(cpu: Float?) = Vista(
        maquinas = listOf(
            VistaDaMaquina("PC", null, Nivel.NORMAL, listOf(Bloco("cpu", "CPU", "x", null, null, Nivel.NORMAL, cpu))),
        ),
        enlace = null,
    )

    @Test
    fun guardaSoOUltimoMinuto() {
        val historico = Historico(tamanho = 3)
        var series = emptyMap<String, List<Float>>()
        for (valor in 1..5) series = historico.anotar(vista(valor.toFloat()))
        assertEquals(listOf(3f, 4f, 5f), series[Historico.chave("PC", "cpu")])
    }

    @Test
    fun umBuracoRecomecaASerie() {
        val historico = Historico()
        historico.anotar(vista(1f))
        historico.anotar(vista(2f))
        assertTrue(historico.anotar(vista(null))[Historico.chave("PC", "cpu")]!!.isEmpty())
        assertEquals(listOf(9f), historico.anotar(vista(9f))[Historico.chave("PC", "cpu")])
    }
}
