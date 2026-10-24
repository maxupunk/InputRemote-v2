package io.github.inputremote.painel.dados

import org.junit.Assert.assertEquals
import org.junit.Test

class ListaEstavelTest {
    private fun linha(nome: String, cpu: Int) =
        LinhaDeProcesso("proc:$nome", nome, 1, "$cpu", "0", cpu, 0, Nivel.NORMAL)

    private fun nomes(linhas: List<LinhaDeProcesso>) = linhas.map { it.nome }

    @Test
    fun aOrdemFicaParadaEnquantoOsValoresMudam() {
        val lista = ListaEstavel(intervaloMs = 5_000)
        assertEquals(listOf("code", "chrome"), nomes(lista.arrumar(listOf(linha("chrome", 10), linha("code", 50)), Ordem.PROCESSADOR, 0)))
        // Um segundo depois o Chrome passou o Code, mas a linha não anda debaixo do dedo.
        val depois = lista.arrumar(listOf(linha("chrome", 90), linha("code", 50), linha("dart", 70)), Ordem.PROCESSADOR, 1_000)
        assertEquals(listOf("code", "chrome", "dart"), nomes(depois))
        assertEquals("o valor é o deste segundo", 90, depois[1].cpuPermille)
    }

    @Test
    fun reordenaNoIntervaloEOuNaHoraAoTrocarDeColuna() {
        val lista = ListaEstavel(intervaloMs = 5_000)
        lista.arrumar(listOf(linha("a", 10), linha("b", 50)), Ordem.PROCESSADOR, 0)
        val trocadas = listOf(linha("a", 90), linha("b", 50))
        assertEquals(listOf("b", "a"), nomes(lista.arrumar(trocadas, Ordem.PROCESSADOR, 4_999)))
        assertEquals(listOf("a", "b"), nomes(lista.arrumar(trocadas, Ordem.PROCESSADOR, 5_000)))
        assertEquals("outra coluna ordena já", listOf("a", "b"), nomes(lista.arrumar(trocadas, Ordem.MEMORIA, 5_001)))
    }

    @Test
    fun quemSumiuSai() {
        val lista = ListaEstavel()
        lista.arrumar(listOf(linha("a", 10), linha("b", 50)), Ordem.PROCESSADOR, 0)
        assertEquals(listOf("a"), nomes(lista.arrumar(listOf(linha("a", 10)), Ordem.PROCESSADOR, 1)))
    }
}
