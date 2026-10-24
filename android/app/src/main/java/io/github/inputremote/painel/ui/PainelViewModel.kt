package io.github.inputremote.painel.ui

import android.app.Application
import android.content.Context
import android.os.Build
import android.provider.Settings
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import io.github.inputremote.painel.nucleo.FonteNativa

/** Segura o [Controlador] através das rotações da tela. Nenhuma regra mora aqui. */
class PainelViewModel(aplicativo: Application) : AndroidViewModel(aplicativo) {
    val controlador = Controlador(fonteNativa(aplicativo), viewModelScope)
    val preferencias = Preferencias(aplicativo)
}

/** A fonte do núcleo em Rust, com a pasta privada do aplicativo e o nome deste aparelho. */
fun fonteNativa(contexto: Context) = FonteNativa(contexto.filesDir.absolutePath, nomeDoAparelho(contexto))

/** O nome que a pessoa deu ao aparelho; sem ele, o modelo. É o que a lista do computador mostra. */
private fun nomeDoAparelho(contexto: Context): String =
    Settings.Global.getString(contexto.contentResolver, "device_name")?.takeIf { it.isNotBlank() }
        ?: Build.MODEL
