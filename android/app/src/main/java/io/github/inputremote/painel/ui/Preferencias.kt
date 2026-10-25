package io.github.inputremote.painel.ui

import android.content.Context
import io.github.inputremote.painel.dados.Arranjo
import io.github.inputremote.painel.dados.ArranjoJson
import io.github.inputremote.painel.dados.PainelPersonalizado
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * O que a pessoa escolhe no aparelho: painéis personalizados, os cards de cada painel,
 * escurecer a tela e girar ou não. Fica no aparelho, e não no computador — é sobre
 * esta tela, e não sobre o que ela mostra.
 */
class Preferencias(contexto: Context) {
    private val arquivo = contexto.getSharedPreferences(ARQUIVO, Context.MODE_PRIVATE)
    private val escurecerAgora = MutableStateFlow(arquivo.getBoolean(ESCURECER, false))
    private val deitadoAgora = MutableStateFlow(arquivo.getBoolean(SEMPRE_DEITADO, true))

    private val paineisAgora = MutableStateFlow(carregarPaineis())
    private val painelAtivoIdAgora = MutableStateFlow(
        arquivo.getString(PAINEL_ATIVO_ID, null)?.takeIf { id -> paineisAgora.value.any { it.id == id } }
            ?: paineisAgora.value.first().id
    )
    private val arranjoAgora = MutableStateFlow(
        paineisAgora.value.firstOrNull { it.id == painelAtivoIdAgora.value }?.arranjo
            ?: paineisAgora.value.first().arranjo
    )

    /** Se a tela fica no brilho mínimo. */
    val escurecer: StateFlow<Boolean> = escurecerAgora.asStateFlow()

    /** Se o painel fica sempre deitado, que é como um tablet ao lado do monitor fica. */
    val sempreDeitado: StateFlow<Boolean> = deitadoAgora.asStateFlow()

    /** Lista de painéis personalizados pelo usuário. */
    val paineis: StateFlow<List<PainelPersonalizado>> = paineisAgora.asStateFlow()

    /** ID do painel atualmente selecionado. */
    val painelAtivoId: StateFlow<String> = painelAtivoIdAgora.asStateFlow()

    /** Quais cards aparecem no painel ativo, em que ordem e quais em destaque. */
    val arranjo: StateFlow<Arranjo> = arranjoAgora.asStateFlow()

    fun alternarEscurecer() = alternar(escurecerAgora, ESCURECER)

    fun alternarDeitado() = alternar(deitadoAgora, SEMPRE_DEITADO)

    /**
     * Seleciona o painel ativo pelo ID.
     */
    fun selecionarPainel(id: String) {
        val painel = paineisAgora.value.firstOrNull { it.id == id } ?: return
        painelAtivoIdAgora.value = id
        arranjoAgora.value = painel.arranjo
        arquivo.edit().putString(PAINEL_ATIVO_ID, id).apply()
    }

    /**
     * Cria um novo painel com o nome especificado e o seleciona.
     */
    fun criarPainel(nome: String): PainelPersonalizado {
        val novoNome = nome.trim().ifEmpty { "Painel ${paineisAgora.value.size + 1}" }
        val novoId = "painel_${System.currentTimeMillis()}"
        val novoPainel = PainelPersonalizado(
            id = novoId,
            nome = novoNome,
            arranjo = Arranjo.PADRAO,
        )
        val novaLista = paineisAgora.value + novoPainel
        paineisAgora.value = novaLista
        selecionarPainel(novoId)
        salvarPaineis(novaLista)
        return novoPainel
    }

    /**
     * Renomeia um painel existente.
     */
    fun renomearPainel(id: String, novoNome: String) {
        val nomeValido = novoNome.trim().ifEmpty { "Painel" }
        val novaLista = paineisAgora.value.map {
            if (it.id == id) it.copy(nome = nomeValido) else it
        }
        if (novaLista != paineisAgora.value) {
            paineisAgora.value = novaLista
            salvarPaineis(novaLista)
        }
    }

    /**
     * Exclui um painel (caso haja mais de um).
     */
    fun excluirPainel(id: String) {
        if (paineisAgora.value.size <= 1) return
        val novaLista = paineisAgora.value.filterNot { it.id == id }
        paineisAgora.value = novaLista
        if (painelAtivoIdAgora.value == id) {
            selecionarPainel(novaLista.first().id)
        }
        salvarPaineis(novaLista)
    }

    /**
     * Atualiza a ordem dos lados das máquinas em um painel.
     */
    fun trocarLadoMaquinas(id: String, novaOrdem: List<String>) {
        val novaLista = paineisAgora.value.map {
            if (it.id == id) it.copy(ordemMaquinas = novaOrdem) else it
        }
        if (novaLista != paineisAgora.value) {
            paineisAgora.value = novaLista
            salvarPaineis(novaLista)
        }
    }

    /**
     * Toda edição dos cards do painel ativo passa por aqui: a tela diz o que muda (`{ it.mover(chave, -1) }`),
     * e o arranjo novo é guardado no painel ativo e persistido.
     */
    fun editarArranjo(edicao: (Arranjo) -> Arranjo) {
        val ativoId = painelAtivoIdAgora.value
        val atual = paineisAgora.value.firstOrNull { it.id == ativoId } ?: paineisAgora.value.first()
        val novoArranjo = edicao(atual.arranjo)
        if (novoArranjo != atual.arranjo) {
            val novaLista = paineisAgora.value.map {
                if (it.id == atual.id) it.copy(arranjo = novoArranjo) else it
            }
            paineisAgora.value = novaLista
            arranjoAgora.value = novoArranjo
            salvarPaineis(novaLista)
            arquivo.edit().putString(ARRANJO, ArranjoJson.escrever(novoArranjo)).apply()
        }
    }

    private fun carregarPaineis(): List<PainelPersonalizado> {
        val doJson = PainelPersonalizado.desserializarLista(arquivo.getString(PAINEIS, null))
        if (!doJson.isNullOrEmpty()) return doJson
        val legado = ArranjoJson.ler(arquivo.getString(ARRANJO, null)) ?: Arranjo.PADRAO
        return listOf(PainelPersonalizado(id = "painel_principal", nome = "Painel", arranjo = legado))
    }

    private fun salvarPaineis(lista: List<PainelPersonalizado>) {
        arquivo.edit().putString(PAINEIS, PainelPersonalizado.serializarLista(lista)).apply()
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
        const val PAINEIS = "paineis_customizados"
        const val PAINEL_ATIVO_ID = "painel_ativo_id"
    }
}
