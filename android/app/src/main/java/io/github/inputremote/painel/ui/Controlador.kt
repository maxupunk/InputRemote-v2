package io.github.inputremote.painel.ui

import io.github.inputremote.painel.dados.ComputadorAchado
import io.github.inputremote.painel.dados.EventoDoNucleo
import io.github.inputremote.painel.dados.FonteDoPainel
import io.github.inputremote.painel.dados.Historico
import io.github.inputremote.painel.dados.Ligacao
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * A máquina de estados do painel: procurar, parear, ligar, mostrar, religar.
 *
 * Só existe ligação enquanto a tela está à vista ([visivel]): ao sair, a ligação fecha, a
 * assinatura acaba, e o computador para de coletar (docs/12, §3.1). Toda espera bloqueante roda em
 * [io]; a tela só lê [tela].
 */
class Controlador(
    private val fonte: FonteDoPainel,
    private val escopo: CoroutineScope,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) {
    private val estado = MutableStateFlow<Tela>(Tela.Abrindo)
    private val historico = Historico()
    private var trabalho: Job? = null

    @Volatile
    private var ligacao: Ligacao? = null

    /** O nome do computador desta ligação, para a tela. */
    @Volatile
    private var nome = ""

    /** O que a tela mostra agora. */
    val tela: StateFlow<Tela> = estado.asStateFlow()

    /** A tela apareceu (`true`) ou sumiu. */
    fun visivel(sim: Boolean) {
        parar()
        if (!sim) return
        iniciar {
            val pareado = fonte.pareados().firstOrNull()
            if (pareado == null) {
                procurarNaRede(null)
            } else {
                nome = pareado.nome
                manter(fonte::voltar)
            }
        }
    }

    /** Procurar computadores na rede, para parear um novo. */
    fun procurar() {
        parar()
        iniciar { procurarNaRede(null) }
    }

    /** Parear com este computador. */
    fun parear(computador: ComputadorAchado) {
        parar()
        historico.limpar()
        iniciar {
            nome = computador.nome
            estado.value = Tela.Conferindo(computador.nome)
            manter { fonte.parear(computador) }
        }
    }

    /** A resposta desta tela ao código. */
    fun confirmar(conferiu: Boolean) {
        ligacao?.confirmar(conferiu)
        if (conferiu) {
            estado.update { (it as? Tela.Conferindo)?.copy(aguardandoOutraTela = true) ?: it }
        } else {
            procurar()
        }
    }

    /** Esquecer o computador de agora e procurar outro. */
    fun esquecer() {
        parar()
        historico.limpar()
        iniciar {
            fonte.pareados().firstOrNull()?.let { fonte.esquecer(it.chave) }
            procurarNaRede(null)
        }
    }

    private fun iniciar(bloco: suspend () -> Unit) {
        trabalho = escopo.launch(io) { bloco() }
    }

    private fun parar() {
        trabalho?.cancel()
        trabalho = null
        ligacao?.fechar()
        ligacao = null
    }

    private fun procurarNaRede(recado: String?) {
        estado.value = Tela.Procurando(procurando = true, recado = recado)
        val achados = fonte.procurar(PROCURA_MS)
        estado.value = Tela.Procurando(achados = achados, recado = recado)
    }

    /**
     * Liga e mantém a ligação, religando depois de cada queda com espera crescente — sempre pela
     * chave gravada, mesmo que a primeira ligação tenha sido um pareamento. Sai quando o computador
     * recusa este aparelho, ou quando um pareamento não chega ao fim.
     */
    private suspend fun manter(primeira: () -> Ligacao?) {
        var abrir = primeira
        var espera = ESPERA_INICIAL_MS
        var pareando = estado.value is Tela.Conferindo
        while (currentCoroutineContext().isActive) {
            val nova = abrir() ?: return procurarNaRede(null)
            ligacao = nova
            nova.assinar(true)
            val queda = acompanhar(nova)
            ligacao = null
            if (queda.recusado) return procurarNaRede(RECUSADO)
            if (pareando && !queda.abriu) return procurarNaRede(queda.motivo)
            pareando = false
            abrir = fonte::voltar
            if (queda.abriu) espera = ESPERA_INICIAL_MS
            estado.value = Tela.Conectando(nome, queda.motivo)
            delay(espera)
            espera = (espera * 2).coerceAtMost(ESPERA_MAXIMA_MS)
        }
    }

    private data class Queda(val motivo: String, val recusado: Boolean, val abriu: Boolean)

    /** Lê os eventos de uma ligação até ela cair. */
    private suspend fun acompanhar(ligacao: Ligacao): Queda {
        var abriu = false
        var ultimoQuadro = System.currentTimeMillis()
        if (estado.value !is Tela.Conferindo) estado.value = Tela.Conectando(nome)
        while (currentCoroutineContext().isActive) {
            when (val evento = ligacao.proximo(PASSO_MS)) {
                is EventoDoNucleo.Codigo -> estado.update { codigo(it, evento.digitos) }
                is EventoDoNucleo.Aberta -> {
                    abriu = true
                    nome = evento.computador
                }
                is EventoDoNucleo.Quadro -> {
                    ultimoQuadro = System.currentTimeMillis()
                    val series = historico.anotar(evento.vista)
                    estado.value = Tela.Painel(nome, evento.vista, series)
                }
                is EventoDoNucleo.Caiu -> return Queda(evento.motivo, evento.recusado, abriu)
                null -> envelhecer(ultimoQuadro)
            }
        }
        return Queda("", recusado = false, abriu = abriu)
    }

    private fun codigo(tela: Tela, digitos: String): Tela =
        (tela as? Tela.Conferindo)?.copy(digitos = digitos) ?: tela

    /** Sem quadro há mais de [VELHO_MS], o painel diz há quanto tempo — e não finge que está em dia. */
    private fun envelhecer(ultimoQuadro: Long) {
        val idade = System.currentTimeMillis() - ultimoQuadro
        if (idade < VELHO_MS) return
        estado.update { (it as? Tela.Painel)?.copy(segundosSemDados = (idade / 1000).toInt()) ?: it }
    }

    private companion object {
        const val PROCURA_MS = 2000
        const val PASSO_MS = 500
        const val VELHO_MS = 3000L
        const val ESPERA_INICIAL_MS = 1000L
        const val ESPERA_MAXIMA_MS = 30_000L
        const val RECUSADO =
            "O computador não reconhece mais este aparelho. Pareie de novo pela janela do InputRemote."
    }
}
