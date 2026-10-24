package io.github.inputremote.painel.ui

import android.content.Context
import io.github.inputremote.painel.dados.Arranjo
import io.github.inputremote.painel.dados.ArranjoJson
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * O que a pessoa escolhe no aparelho: os cards do painel, escurecer a tela e girar ou não. Fica no
 * aparelho, e não no computador — é sobre esta tela, e não sobre o que ela mostra.
 */
class Preferencias(contexto: Context) {
    private val arquivo = contexto.getSharedPreferences(ARQUIVO, Context.MODE_PRIVATE)
    private val escurecerAgora = MutableStateFlow(arquivo.getBoolean(ESCURECER, false))
    private val deitadoAgora = MutableStateFlow(arquivo.getBoolean(SEMPRE_DEITADO, true))
    private val arranjoAgora = MutableStateFlow(ArranjoJson.ler(arquivo.getString(ARRANJO, null)) ?: Arranjo.PADRAO)

    /** Se a tela fica no brilho mínimo. */
    val escurecer: StateFlow<Boolean> = escurecerAgora.asStateFlow()

    /** Se o painel fica sempre deitado, que é como um tablet ao lado do monitor fica. */
    val sempreDeitado: StateFlow<Boolean> = deitadoAgora.asStateFlow()

    /** Quais cards aparecem, em que ordem e quais em destaque. */
    val arranjo: StateFlow<Arranjo> = arranjoAgora.asStateFlow()

    fun alternarEscurecer() = alternar(escurecerAgora, ESCURECER)

    fun alternarDeitado() = alternar(deitadoAgora, SEMPRE_DEITADO)

    /**
     * Toda edição dos cards passa por aqui: a tela diz o que muda (`{ it.mover(chave, -1) }`), e o
     * arranjo novo é guardado — só se mudou de fato.
     */
    fun editarArranjo(edicao: (Arranjo) -> Arranjo) {
        val antes = arranjoAgora.value
        arranjoAgora.update(edicao)
        val depois = arranjoAgora.value
        if (depois != antes) arquivo.edit().putString(ARRANJO, ArranjoJson.escrever(depois)).apply()
    }

    private fun alternar(estado: MutableStateFlow<Boolean>, chave: String) {
        val novo = !estado.value
        arquivo.edit().putBoolean(chave, novo).apply()
        estado.value = novo
    }

    private companion object {
        const val ARQUIVO = "painel"
        const val ESCURECER = "escurecer"
        const val SEMPRE_DEITADO = "sempre-deitado"
        const val ARRANJO = "arranjo"
    }
}
