package io.github.inputremote.painel.dados

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ArranjoJsonTest {
    @Test
    fun idaEVolta() {
        val arranjo = Arranjo.PADRAO.remover("disco").alternarDestaque("rede").mover("rede", -2)
        assertEquals(arranjo, ArranjoJson.ler(ArranjoJson.escrever(arranjo)))
    }

    @Test
    fun textoEstragadoNaoEArranjo() {
        assertNull(ArranjoJson.ler(null))
        assertNull(ArranjoJson.ler("{"))
        assertNull(ArranjoJson.ler("""{"cards":3}"""))
    }
}
