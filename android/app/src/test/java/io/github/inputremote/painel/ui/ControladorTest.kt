package io.github.inputremote.painel.ui

import io.github.inputremote.painel.dados.ComputadorAchado
import io.github.inputremote.painel.dados.ComputadorPareado
import io.github.inputremote.painel.dados.EventoDoNucleo
import io.github.inputremote.painel.dados.FonteDoPainel
import io.github.inputremote.painel.dados.Ligacao
import io.github.inputremote.painel.dados.Vista
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Uma ligação de mentira: os eventos vêm de uma fila que o teste enche. */
private class LigacaoFalsa : Ligacao {
    val eventos = LinkedBlockingQueue<EventoDoNucleo>()
    val confirmacoes = mutableListOf<Boolean>()
    var assinada = false
    var fechada = false

    override fun proximo(esperaMs: Int): EventoDoNucleo? = eventos.poll(esperaMs.toLong(), TimeUnit.MILLISECONDS)
    override fun confirmar(conferiu: Boolean) {
        confirmacoes += conferiu
    }
    override fun assinar(quer: Boolean) {
        assinada = quer
    }
    override fun fechar() {
        fechada = true
    }
}

private class FonteFalsa(
    var pareados: List<ComputadorPareado> = emptyList(),
    val achados: List<ComputadorAchado> = listOf(ComputadorAchado("DESKTOP", "10.0.0.2")),
) : FonteDoPainel {
    val ligacoes = LinkedBlockingQueue<LigacaoFalsa>()
    override fun procurar(duracaoMs: Int) = achados
    override fun pareados() = pareados
    override fun esquecer(chave: String) {
        pareados = pareados.filterNot { it.chave == chave }
    }
    override fun parear(computador: ComputadorAchado): Ligacao = nova()
    override fun voltar(): Ligacao? = if (pareados.isEmpty()) null else nova()
    private fun nova() = LigacaoFalsa().also { ligacoes += it }
}

class ControladorTest {
    private val escopo = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val pareado = ComputadorPareado("DESKTOP", "ab", "10.0.0.2:52527")
    private val vista = Vista(maquinas = emptyList(), enlace = null)

    @After
    fun fim() = escopo.cancel()

    private fun esperarTela(controlador: Controlador, condicao: (Tela) -> Boolean): Tela = runBlocking {
        withTimeout(5_000) { controlador.tela.first(condicao) }
    }

    private fun proximaLigacao(fonte: FonteFalsa) = fonte.ligacoes.poll(5, TimeUnit.SECONDS)!!

    @Test
    fun semPareadoProcuraNaRede() {
        val controlador = Controlador(FonteFalsa(), escopo)
        controlador.visivel(true)
        val tela = esperarTela(controlador) { it is Tela.Procurando && !it.procurando } as Tela.Procurando
        assertEquals("DESKTOP", tela.achados.single().nome)
    }

    @Test
    fun comPareadoLigaAssinaEMostraOQuadro() {
        val fonte = FonteFalsa(pareados = listOf(pareado))
        val controlador = Controlador(fonte, escopo)
        controlador.visivel(true)
        val ligacao = proximaLigacao(fonte)
        ligacao.eventos += EventoDoNucleo.Aberta("DESKTOP", pareou = false)
        ligacao.eventos += EventoDoNucleo.Quadro(vista)
        val tela = esperarTela(controlador) { it is Tela.Painel } as Tela.Painel
        assertEquals("DESKTOP", tela.computador)
        assertTrue("o painel assina ao ligar", ligacao.assinada)

        controlador.visivel(false)
        assertTrue("a tela sumiu, a ligação fecha", ligacao.fechada)
    }

    @Test
    fun parearMostraOCodigoEsperaAOutraTelaEAbre() {
        val fonte = FonteFalsa()
        val controlador = Controlador(fonte, escopo)
        controlador.parear(ComputadorAchado("DESKTOP", "10.0.0.2"))
        val ligacao = proximaLigacao(fonte)
        ligacao.eventos += EventoDoNucleo.Codigo("275801")
        esperarTela(controlador) { it is Tela.Conferindo && it.digitos == "275801" }

        controlador.confirmar(true)
        assertEquals(listOf(true), ligacao.confirmacoes)
        esperarTela(controlador) { it is Tela.Conferindo && it.aguardandoOutraTela }

        ligacao.eventos += EventoDoNucleo.Aberta("DESKTOP", pareou = true)
        ligacao.eventos += EventoDoNucleo.Quadro(vista)
        esperarTela(controlador) { it is Tela.Painel }
    }

    @Test
    fun recusadoVoltaAProcuraDizendoPorque() {
        val fonte = FonteFalsa(pareados = listOf(pareado))
        val controlador = Controlador(fonte, escopo)
        controlador.visivel(true)
        proximaLigacao(fonte).eventos += EventoDoNucleo.Caiu("recusado", recusado = true)
        val tela = esperarTela(controlador) { it is Tela.Procurando && !it.procurando } as Tela.Procurando
        assertTrue(tela.recado!!.contains("Pareie de novo"))
    }

    @Test
    fun umaQuedaComumReligaSozinha() {
        val fonte = FonteFalsa(pareados = listOf(pareado))
        val controlador = Controlador(fonte, escopo)
        controlador.visivel(true)
        proximaLigacao(fonte).eventos += EventoDoNucleo.Caiu("a rede caiu", recusado = false)
        val tela = esperarTela(controlador) { it is Tela.Conectando && it.motivo != null } as Tela.Conectando
        assertEquals("a rede caiu", tela.motivo)
        // Depois da espera, uma ligação nova — sem a pessoa fazer nada.
        proximaLigacao(fonte)
    }
}
