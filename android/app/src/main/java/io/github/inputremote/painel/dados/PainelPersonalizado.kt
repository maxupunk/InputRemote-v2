package io.github.inputremote.painel.dados

import org.json.JSONArray
import org.json.JSONObject

/**
 * Um painel configurado pelo usuário: identificador único, nome exibido na aba,
 * arranjo exclusivo de cards e ordem personalizada das máquinas.
 *
 * Imutável: cada alteração devolve uma nova instância.
 */
data class PainelPersonalizado(
    val id: String,
    val nome: String,
    val arranjo: Arranjo = Arranjo.PADRAO,
    val ordemMaquinas: List<String> = emptyList(),
) {
    /**
     * Retorna as máquinas na ordem definida para este painel.
     * Se não houver ordem gravada ou menos de 2 máquinas, mantém a ordem original recebida.
     */
    fun ordenarMaquinas(maquinas: List<VistaDaMaquina>): List<VistaDaMaquina> {
        if (ordemMaquinas.isEmpty() || maquinas.size < 2) return maquinas
        val ordemIndex = ordemMaquinas.withIndex().associate { it.value.trim().lowercase() to it.index }
        return maquinas.sortedBy { ordemIndex[it.nome.trim().lowercase()] ?: Int.MAX_VALUE }
    }

    /**
     * Inverte a ordem das máquinas atualmente exibidas neste painel.
     */
    fun inverterLados(maquinas: List<VistaDaMaquina>): PainelPersonalizado {
        if (maquinas.size < 2) return this
        val ordenadas = ordenarMaquinas(maquinas)
        val novaOrdem = ordenadas.reversed().map { it.nome }
        return copy(ordemMaquinas = novaOrdem)
    }

    companion object {
        val PADRAO = PainelPersonalizado(
            id = "painel_principal",
            nome = "Painel",
            arranjo = Arranjo.PADRAO,
        )

        /** Serializa a lista de painéis para JSON persistível em SharedPreferences. */
        fun serializarLista(paineis: List<PainelPersonalizado>): String {
            val array = JSONArray()
            paineis.forEach { painel ->
                val obj = JSONObject()
                    .put("id", painel.id)
                    .put("nome", painel.nome)
                    .put("arranjo", JSONObject(ArranjoJson.escrever(painel.arranjo)))
                    .put("ordemMaquinas", JSONArray(painel.ordemMaquinas))
                array.put(obj)
            }
            return array.toString()
        }

        /** Desserializa a lista de painéis a partir de JSON. Em caso de falha, retorna nulo. */
        fun desserializarLista(texto: String?): List<PainelPersonalizado>? = runCatching {
            val array = JSONArray(texto ?: return null)
            if (array.length() == 0) return null
            List(array.length()) { i ->
                val obj = array.getJSONObject(i)
                val id = obj.getString("id")
                val nome = obj.getString("nome")
                val arranjoStr = obj.optJSONObject("arranjo")?.toString()
                val arranjo = ArranjoJson.ler(arranjoStr) ?: Arranjo.PADRAO
                val ordemArr = obj.optJSONArray("ordemMaquinas")
                val ordem = if (ordemArr != null) {
                    List(ordemArr.length()) { idx -> ordemArr.getString(idx) }
                } else {
                    emptyList()
                }
                PainelPersonalizado(id, nome, arranjo, ordem)
            }
        }.getOrNull()
    }
}

