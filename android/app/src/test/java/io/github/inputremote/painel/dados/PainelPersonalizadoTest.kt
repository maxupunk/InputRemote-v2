package io.github.inputremote.painel.dados

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
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
    fun cadaComputadorTemOsSeusCards() {
        val painel = PainelPersonalizado(id = "p1", nome = "Painel")
            .editar("Fedora") { it.remover("disco") }

        assertEquals(false, painel.arranjoDe("fedora").visivel("disco"))
        // O outro computador continua com o arranjo do painel, intacto.
        assertEquals(true, painel.arranjoDe("SAMSUNG").visivel("disco"))
        assertEquals(Arranjo.PADRAO, painel.arranjoDe(null))
    }

    @Test
    fun editarSemMudarNadaDevolveOMesmoPainel() {
        val painel = PainelPersonalizado(id = "p1", nome = "Painel")
        assertSame(painel, painel.editar("fedora") { it.mover("cpu", -1) })
    }

    @Test
    fun chaveNovaEntraNoComputadorQueAMandou() {
        val painel = PainelPersonalizado(id = "p1", nome = "Painel")
            .editar("fedora") { it.remover("disco") }
            .conhecer(mapOf("fedora" to listOf("cpu", "temp-nvme"), "samsung" to listOf("bateria-2")), listOf(Arranjo.COPIA))

        assertEquals(true, painel.arranjoDe("fedora").visivel("temp-nvme"))
        assertEquals(false, painel.arranjoDe("fedora").visivel("bateria-2"))
        // O do painel conhece todas: é o ponto de partida de quem ainda não foi arrumado.
        assertEquals(true, painel.arranjoDe("samsung").visivel("bateria-2"))
    }

    @Test
    fun restaurarVoltaOsDoisComputadores() {
        val painel = PainelPersonalizado(id = "p1", nome = "Painel")
            .editar("fedora") { it.remover("disco") }
            .editar(null) { it.remover(Arranjo.ENLACE) }
            .restaurado()

        assertEquals(emptyMap<String, Arranjo>(), painel.porMaquina)
        assertEquals(true, painel.arranjoDe(null).visivel(Arranjo.ENLACE))
    }

    @Test
    fun processosDeTodosOsComputadores() {
        val painel = PainelPersonalizado(id = "p1", nome = "Painel")
            .editar("fedora") { it.acompanhar("docker") }
            .editar("samsung") { it.acompanhar("chrome") }

        assertEquals(setOf("docker", "chrome"), painel.processos().toSet())
    }

    @Test
    fun osCardsDeCadaComputadorSobrevivemAoJson() {
        val painel = PainelPersonalizado(id = "p1", nome = "Painel")
            .editar("fedora") { it.remover("disco").alternarDestaque("rede") }

        val lido = PainelPersonalizado.desserializarLista(PainelPersonalizado.serializarLista(listOf(painel)))!!.single()

        assertEquals(painel, lido)
    }

    @Test
    fun desserializacaoInvalidaRetornaNulo() {
        assertNull(PainelPersonalizado.desserializarLista(null))
        assertNull(PainelPersonalizado.desserializarLista(""))
        assertNull(PainelPersonalizado.desserializarLista("[]"))
        assertNull(PainelPersonalizado.desserializarLista("invalid json"))
    }
}

