package io.github.inputremote.painel.dados

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class PainelPersonalizadoTest {

    private fun criarMaquina(nome: String) = VistaDaMaquina(
        nome = nome,
        nivel = Nivel.NORMAL,
        aviso = null,
        blocos = emptyList(),
    )

    @Test
    fun ordenacaoPadraoSemOrdemPersonalizadaMantemOriginal() {
        val m1 = criarMaquina("bancada")
        val m2 = criarMaquina("notebook (simulado)")
        val painel = PainelPersonalizado(id = "p1", nome = "Painel")

        val ordenadas = painel.ordenarMaquinas(listOf(m1, m2))
        assertEquals(listOf("bancada", "notebook (simulado)"), ordenadas.map { it.nome })
    }

    @Test
    fun inverterLadosAlternaOrdemDasMaquinas() {
        val m1 = criarMaquina("bancada")
        val m2 = criarMaquina("notebook (simulado)")
        val painel = PainelPersonalizado(id = "p1", nome = "Painel")

        val invertido = painel.inverterLados(listOf(m1, m2))
        assertEquals(listOf("notebook (simulado)", "bancada"), invertido.ordemMaquinas)

        val ordenadas = invertido.ordenarMaquinas(listOf(m1, m2))
        assertEquals(listOf("notebook (simulado)", "bancada"), ordenadas.map { it.nome })

        // Inverter de novo volta ao original
        val reinvertido = invertido.inverterLados(listOf(m1, m2))
        assertEquals(listOf("bancada", "notebook (simulado)"), reinvertido.ordemMaquinas)
    }

    @Test
    fun serializacaoEDesserializacaoDeLista() {
        val paineis = listOf(
            PainelPersonalizado(
                id = "p1",
                nome = "Geral",
                arranjo = Arranjo.PADRAO.remover("disco"),
                ordemMaquinas = listOf("notebook (simulado)", "bancada"),
            ),
            PainelPersonalizado(
                id = "p2",
                nome = "Hardware",
                arranjo = Arranjo.PADRAO.alternarDestaque("rede"),
                ordemMaquinas = emptyList(),
            ),
        )

        val json = PainelPersonalizado.serializarLista(paineis)
        val lidos = PainelPersonalizado.desserializarLista(json)

        assertNotNull(lidos)
        assertEquals(2, lidos!!.size)
        assertEquals("p1", lidos[0].id)
        assertEquals("Geral", lidos[0].nome)
        assertEquals(listOf("notebook (simulado)", "bancada"), lidos[0].ordemMaquinas)
        assertEquals(paineis[0].arranjo, lidos[0].arranjo)

        assertEquals("p2", lidos[1].id)
        assertEquals("Hardware", lidos[1].nome)
        assertEquals(emptyList<String>(), lidos[1].ordemMaquinas)
        assertEquals(paineis[1].arranjo, lidos[1].arranjo)
    }

    @Test
    fun desserializacaoInvalidaRetornaNulo() {
        assertNull(PainelPersonalizado.desserializarLista(null))
        assertNull(PainelPersonalizado.desserializarLista(""))
        assertNull(PainelPersonalizado.desserializarLista("[]"))
        assertNull(PainelPersonalizado.desserializarLista("invalid json"))
    }
}

