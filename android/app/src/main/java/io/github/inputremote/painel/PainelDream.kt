package io.github.inputremote.painel

import android.service.dreams.DreamService
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.ComposeView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.setViewTreeLifecycleOwner
import androidx.savedstate.SavedStateRegistry
import androidx.savedstate.SavedStateRegistryController
import androidx.savedstate.SavedStateRegistryOwner
import androidx.savedstate.setViewTreeSavedStateRegistryOwner
import io.github.inputremote.painel.ui.Controlador
import io.github.inputremote.painel.ui.Preferencias
import io.github.inputremote.painel.ui.fonteNativa
import io.github.inputremote.painel.ui.telas.Acoes
import io.github.inputremote.painel.ui.telas.Aparelho
import io.github.inputremote.painel.ui.telas.Painel
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.cancel

/**
 * O painel como protetor de tela: o Android o abre sozinho ao carregar, ou na base
 * (docs/12, §5.3). A mesma tela da [MainActivity], só para olhar — um toque acorda o aparelho.
 *
 * Um `DreamService` não é uma `Activity`: o ciclo de vida que o Compose precisa é montado aqui, e
 * acompanha o sonho — ligação aberta enquanto ele está na tela, fechada quando ele acaba.
 */
class PainelDream : DreamService(), LifecycleOwner, SavedStateRegistryOwner {
    private val ciclo = LifecycleRegistry(this)
    private val salvamento = SavedStateRegistryController.create(this)
    private var escopo: CoroutineScope? = null
    private var controlador: Controlador? = null

    override val lifecycle: Lifecycle get() = ciclo
    override val savedStateRegistry: SavedStateRegistry get() = salvamento.savedStateRegistry

    override fun onCreate() {
        super.onCreate()
        salvamento.performRestore(null)
        ciclo.currentState = Lifecycle.State.CREATED
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        isInteractive = false
        isFullscreen = true
        val preferencias = Preferencias(this)
        isScreenBright = !preferencias.escurecer.value
        val escopo = MainScope().also { escopo = it }
        val controlador = Controlador(fonteNativa(this), escopo).also { controlador = it }
        val nada = {}
        val acoes = Acoes(procurar = nada, parear = {}, confirmar = {}, esquecer = nada)
        // Os mesmos cards que a pessoa arrumou no aplicativo; aqui só para olhar.
        val aparelho = Aparelho(
            arranjo = preferencias.arranjo.value,
            editarArranjo = {},
            escurecido = !isScreenBright,
            alternarEscurecer = nada,
            sempreDeitado = preferencias.sempreDeitado.value,
            alternarDeitado = nada,
            editavel = false,
        )
        setContentView(
            ComposeView(this).apply {
                setViewTreeLifecycleOwner(this@PainelDream)
                setViewTreeSavedStateRegistryOwner(this@PainelDream)
                setContent {
                    val tela by controlador.tela.collectAsStateWithLifecycle()
                    Painel(tela, acoes, aparelho)
                }
            },
        )
    }

    override fun onDreamingStarted() {
        super.onDreamingStarted()
        ciclo.currentState = Lifecycle.State.RESUMED
        controlador?.visivel(true)
    }

    override fun onDreamingStopped() {
        controlador?.visivel(false)
        ciclo.currentState = Lifecycle.State.CREATED
        super.onDreamingStopped()
    }

    override fun onDetachedFromWindow() {
        controlador?.visivel(false)
        escopo?.cancel()
        ciclo.currentState = Lifecycle.State.DESTROYED
        super.onDetachedFromWindow()
    }
}
