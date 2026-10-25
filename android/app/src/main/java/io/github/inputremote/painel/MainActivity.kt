package io.github.inputremote.painel

import android.content.pm.ActivityInfo
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.inputremote.painel.ui.PainelViewModel
import io.github.inputremote.painel.ui.telas.Acoes
import io.github.inputremote.painel.ui.telas.Aparelho
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
            val deitado by preferencias.sempreDeitado.collectAsStateWithLifecycle()
            val arranjo by preferencias.arranjo.collectAsStateWithLifecycle()
            val paineis by preferencias.paineis.collectAsStateWithLifecycle()
            val painelAtivoId by preferencias.painelAtivoId.collectAsStateWithLifecycle()
            LaunchedEffect(escuro) { aplicarBrilho(escuro) }
            LaunchedEffect(deitado) { aplicarOrientacao(deitado) }
            // Os cards de processo são o pedido ao computador: o que está nos painéis é o que ele mede.
            // Se houver vagas (até 4), inclui processos populares como chrome ou o processo pesquisado.
            var processoPesquisado by remember { mutableStateOf<String?>(null) }
            val processos = remember(paineis, processoPesquisado) {
                val dosPaineis = paineis.flatMap { it.arranjo.processos() }.distinct()
                val pesquisa = listOfNotNull(processoPesquisado?.takeIf { it.isNotBlank() })
                val populares = listOf("chrome", "code", "docker", "wsl")
                (pesquisa + dosPaineis + populares).distinct().take(io.github.inputremote.painel.dados.Arranjo.MAX_PROCESSOS)
            }
            LaunchedEffect(processos) { controlador.acompanhar(processos) }
            Painel(
                tela,
                Acoes(
                    procurar = controlador::procurar,
                    parear = controlador::parear,
                    confirmar = controlador::confirmar,
                    esquecer = controlador::esquecer,
                    abrirLista = controlador::abrirLista,
                    acompanhar = controlador::acompanhar,
                    pesquisarProcesso = { processoPesquisado = it },
                ),
                Aparelho(
                    arranjo = arranjo,
                    editarArranjo = preferencias::editarArranjo,
                    escurecido = escuro,
                    alternarEscurecer = preferencias::alternarEscurecer,
                    sempreDeitado = deitado,
                    alternarDeitado = preferencias::alternarDeitado,
                    paineis = paineis,
                    painelAtivoId = painelAtivoId,
                    aoSelecionarPainel = preferencias::selecionarPainel,
                    aoCriarPainel = { preferencias.criarPainel(it) },
                    aoRenomearPainel = preferencias::renomearPainel,
                    aoExcluirPainel = preferencias::excluirPainel,
                    aoTrocarLadoMaquinas = preferencias::trocarLadoMaquinas,
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

    /**
     * Deitado nos dois sentidos (o carregador pode estar de qualquer lado); ou livre, girando com o
     * aparelho.
     */
    private fun aplicarOrientacao(deitado: Boolean) {
        requestedOrientation = if (deitado) {
            ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        } else {
            ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
        }
    }

    private companion object {
        /** Legível num quarto escuro, e bem menos luz que o automático. */
        const val BRILHO_BAIXO = 0.05f
    }
}
