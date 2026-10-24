package io.github.inputremote.painel.dados

import org.junit.Assert.assertEquals
import org.junit.Test

class OrdemTest {
    private fun linha(nome: String, cpu: Int, memoria: Long) =
        LinhaDeProcesso("proc:$nome", nome, 1, "$cpu", "$memoria", cpu, memoria, Nivel.NORMAL)

    private val linhas = listOf(linha("chrome", 50, 900), linha("rustc", 700, 300), linha("vmmem", 50, 4000))

    @Test
    fun cadaColunaOrdenaDoMaisPesadoParaOMaisLeve() {
        assertEquals(listOf("rustc", "chrome", "vmmem"), Ordem.PROCESSADOR.ordenar(linhas).map { it.nome })
        assertEquals(listOf("vmmem", "chrome", "rustc"), Ordem.MEMORIA.ordenar(linhas).map { it.nome })
    }

    @Test
    fun aBarraDoProcessadorEDaMaquinaEADaMemoriaDoMaisPesado() {
        assertEquals(0.7f, Ordem.PROCESSADOR.fracao(linhas[1], linhas), 0.0001f)
        assertEquals(1f, Ordem.MEMORIA.fracao(linhas[2], linhas), 0.0001f)
        assertEquals(0.225f, Ordem.MEMORIA.fracao(linhas[0], linhas), 0.0001f)
    }

    @Test
    fun oIDaMemoriaAbrePelaMemoria() {
        assertEquals(Ordem.MEMORIA, Ordem.doCard("ram"))
        assertEquals(Ordem.PROCESSADOR, Ordem.doCard("cpu"))
    }
}
