package io.github.inputremote.painel.ui

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * O que a pessoa escolhe no aparelho: por ora, só escurecer a tela. Fica no aparelho, e não no
 * computador — é sobre esta tela, e não sobre o que ela mostra.
 */
class Preferencias(contexto: Context) {
    private val arquivo = contexto.getSharedPreferences(ARQUIVO, Context.MODE_PRIVATE)
    private val escurecerAgora = MutableStateFlow(arquivo.getBoolean(ESCURECER, false))

    /** Se a tela fica no brilho mínimo. */
    val escurecer: StateFlow<Boolean> = escurecerAgora.asStateFlow()

    fun alternarEscurecer() {
        val novo = !escurecerAgora.value
        arquivo.edit().putBoolean(ESCURECER, novo).apply()
        escurecerAgora.value = novo
    }

    private companion object {
        const val ARQUIVO = "painel"
        const val ESCURECER = "escurecer"
    }
}
