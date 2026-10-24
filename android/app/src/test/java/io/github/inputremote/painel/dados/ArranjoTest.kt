package io.github.inputremote.painel.dados

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class ArranjoTest {
    private val padrao = Arranjo.PADRAO
    private fun chaves(arranjo: Arranjo) = arranjo.cards.map { it.chave }

    @Test
    fun cpuEMemoriaVemPrimeiroEmDestaque() {
        assertEquals(listOf("cpu", "ram"), chaves(padrao).take(2))
        assertTrue(padrao.cards.take(2).all { it.destaque })
        assertTrue(padrao.cards.drop(2).none { it.destaque })
    }

    @Test
    fun moverAndaEParaNosExtremos() {
        assertEquals(listOf("ram", "cpu"), chaves(padrao.mover("ram", -1)).take(2))
        assertSame(padrao, padrao.mover("cpu", -1))
        assertEquals("cpu", chaves(padrao.mover("cpu", 99)).last())
        assertSame(padrao, padrao.mover("nao-existe", 1))
    }

    @Test
    fun removerEsconderMasLembraParaAdicionarDeNovo() {
        val sem = padrao.remover("disco")
        assertFalse(sem.visivel("disco"))
        assertEquals(setOf("disco"), sem.ocultas())
        val com = sem.adicionar("disco")
        assertEquals("disco", chaves(com).last())
        assertTrue(com.ocultas().isEmpty())
        assertSame(com, com.adicionar("disco"))
    }

    @Test
    fun umaMetricaNovaEntraNoFimEUmaApagadaNaoVolta() {
        val sem = padrao.remover("rede")
        val depois = sem.conhecer(listOf("rede", "cpu", "fan"))
        assertEquals("fan", chaves(depois).last())
        assertFalse("apagada pela pessoa continua apagada", depois.visivel("rede"))
        assertSame(depois, depois.conhecer(listOf("fan")))
    }

    @Test
    fun destaqueAlterna() {
        val arranjo = padrao.alternarDestaque("cpu").alternarDestaque("rede")
        assertFalse(arranjo.cards.first { it.chave == "cpu" }.destaque)
        assertTrue(arranjo.cards.first { it.chave == "rede" }.destaque)
    }

    @Test
    fun ordenarSegueOArranjoEDeixaDeForaOQueNaoEstaNele() {
        val itens = listOf("disco", "zzz", "ram", "cpu")
        val ordenados = padrao.remover("disco").ordenar(itens) { it }
        assertEquals(listOf("cpu", "ram"), ordenados.map { it.first })
    }

    @Test
    fun acompanharUmProcessoCriaOCardEPedeAoComputador() {
        val arranjo = padrao.acompanhar(" Docker ").acompanhar("wsl")
        assertEquals(listOf("proc:docker", "proc:wsl"), chaves(arranjo).takeLast(2))
        assertEquals(listOf("docker", "wsl"), arranjo.processos())
        assertSame("o mesmo processo não entra duas vezes", arranjo, arranjo.acompanhar("DOCKER"))
        assertSame("sem nome, nada", arranjo, arranjo.acompanhar("  "))
    }

    @Test
    fun cabemQuatroProcessos() {
        val cheio = listOf("a", "b", "c", "d").fold(padrao) { arranjo, nome -> arranjo.acompanhar(nome) }
        assertFalse(cheio.podeAcompanhar())
        assertSame(cheio, cheio.acompanhar("e"))
    }

    @Test
    fun processoTiradoEEsquecidoEOdeOutroPainelNaoEntra() {
        val sem = padrao.acompanhar("docker").remover("proc:docker")
        assertTrue(sem.processos().isEmpty())
        assertFalse("proc:docker" in sem.conhecidas)
        assertSame(sem, sem.conhecer(listOf("proc:steam")))
    }
}
