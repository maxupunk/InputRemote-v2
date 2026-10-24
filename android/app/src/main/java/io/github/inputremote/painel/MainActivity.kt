package io.github.inputremote.painel

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.inputremote.painel.ui.PainelViewModel
import io.github.inputremote.painel.ui.telas.Acoes
import io.github.inputremote.painel.ui.telas.Painel

/**
 * A tela do painel.
 *
 * Fica acesa enquanto está à vista, e só então existe ligação com o computador: ao sair da tela a
 * ligação fecha e o computador para de coletar (docs/12, §5.1). Sem serviço em primeiro plano e sem
 * nada em segundo plano.
 */
class MainActivity : ComponentActivity() {
    private val modelo: PainelViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        esconderAsBarras()
        val controlador = modelo.controlador
        val preferencias = modelo.preferencias
        setContent {
            val tela by controlador.tela.collectAsStateWithLifecycle()
            val escuro by preferencias.escurecer.collectAsStateWithLifecycle()
            LaunchedEffect(escuro) { aplicarBrilho(escuro) }
            Painel(
                tela,
                Acoes(
                    procurar = controlador::procurar,
                    parear = controlador::parear,
                    confirmar = controlador::confirmar,
                    esquecer = controlador::esquecer,
                    escurecido = escuro,
                    alternarEscurecer = preferencias::alternarEscurecer,
                ),
            )
        }
    }

    override fun onStart() {
        super.onStart()
        modelo.controlador.visivel(true)
    }

    override fun onStop() {
        modelo.controlador.visivel(false)
        super.onStop()
    }

    /** Tela cheia: as barras do sistema aparecem num deslize e somem sozinhas. */
    private fun esconderAsBarras() {
        WindowCompat.getInsetsController(window, window.decorView).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
    }

    private fun aplicarBrilho(escuro: Boolean) {
        window.attributes = window.attributes.apply {
            screenBrightness = if (escuro) BRILHO_BAIXO else WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
        }
    }

    private companion object {
        /** Legível num quarto escuro, e bem menos luz que o automático. */
        const val BRILHO_BAIXO = 0.05f
    }
}
