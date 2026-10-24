package io.github.inputremote.painel.nucleo

import io.github.inputremote.painel.dados.EventoDoNucleo
import io.github.inputremote.painel.dados.Nivel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** O JSON como o núcleo o escreve (`crates/ir-monitor-android/src/ponte.rs`). */
class LeituraTest {
    private val quadro = """
        {"tipo":"quadro","vista":{"maquinas":[{"nome":"DESKTOP","aviso":null,"nivel":"atencao",
        "blocos":[{"chave":"cpu","rotulo":"CPU","valor":"23 %","detalhe":null,"fracao":0.231,
        "nivel":"normal","serie":23.1},{"chave":"temp","rotulo":"Temperatura","valor":"—",
        "detalhe":"sem sensor legível","fracao":null,"nivel":"sem_dado","serie":null}]},
        {"nome":"notebook","aviso":"Aguardando os dados…","nivel":"normal","blocos":[]}],
        "enlace":{"rotas":[{"chave":"bluetooth","rotulo":"Bluetooth","valor":"7,0 ms","detalhe":null,
        "fracao":null,"nivel":"normal","serie":7.0}],"vencendo":null}}}
    """.trimIndent()

    @Test
    fun oQuadroViraAVistaInteira() {
        val evento = Leitura.evento(quadro) as EventoDoNucleo.Quadro
        val desktop = evento.vista.maquinas[0]
        assertEquals("DESKTOP", desktop.nome)
        assertEquals(Nivel.ATENCAO, desktop.nivel)
        assertNull(desktop.aviso)
        assertEquals(0.231f, desktop.blocos[0].fracao!!, 0.0001f)
        assertEquals(Nivel.SEM_DADO, desktop.blocos[1].nivel)
        assertNull(desktop.blocos[1].serie)
        assertEquals("sem sensor legível", desktop.blocos[1].detalhe)
        assertEquals("Aguardando os dados…", evento.vista.maquinas[1].aviso)
        assertEquals(7.0f, evento.vista.enlace!!.rotas[0].serie!!, 0.0001f)
        assertNull(evento.vista.enlace!!.vencendo)
    }

    @Test
    fun osOutrosEventos() {
        assertEquals(EventoDoNucleo.Codigo("041907"), Leitura.evento("""{"tipo":"codigo","digitos":"041907"}"""))
        assertEquals(
            EventoDoNucleo.Aberta("DESKTOP", true),
            Leitura.evento("""{"tipo":"aberta","computador":"DESKTOP","pareou":true}"""),
        )
        assertEquals(
            EventoDoNucleo.Caiu("x", true),
            Leitura.evento("""{"tipo":"caiu","motivo":"x","recusado":true}"""),
        )
    }

    @Test
    fun semEventoOuEventoDesconhecidoENulo() {
        assertNull(Leitura.evento(""))
        assertNull(Leitura.evento("""{"tipo":"de-uma-versao-nova"}"""))
    }

    @Test
    fun asListas() {
        val achados = Leitura.achados("""[{"nome":"PC","endereco":"10.0.0.2"}]""")
        assertEquals("10.0.0.2", achados.single().endereco)
        assertTrue(Leitura.pareados("[]").isEmpty())
    }

    @Test
    fun aListaDeProcessosEACopia() {
        val texto = """
            {"tipo":"quadro","vista":{"maquinas":[{"nome":"PC","aviso":null,"nivel":"normal","blocos":[],
            "processos":[{"chave":"proc:docker","nome":"docker","instancias":3,"cpu":"1,5 %",
            "memoria":"2,0 GB","cpu_permille":15,"memoria_kib":2097152,"nivel":"normal"}]},
            {"nome":"nb","aviso":null,"nivel":"normal","blocos":[],"processos":null}],
            "enlace":null,"copia":{"chave":"copia","rotulo":"Cópia · enviando","valor":"40 %",
            "detalhe":"fotos","fracao":0.4,"nivel":"normal","serie":2.0}}}
        """.trimIndent()
        val vista = (Leitura.evento(texto) as EventoDoNucleo.Quadro).vista
        val docker = vista.maquinas[0].processos!!.single()
        assertEquals("proc:docker", docker.chave)
        assertEquals(3, docker.instancias)
        assertEquals(2_097_152L, docker.memoriaKib)
        assertNull("sem lista pedida, nula", vista.maquinas[1].processos)
        assertEquals("40 %", vista.copia!!.valor)
    }
}
