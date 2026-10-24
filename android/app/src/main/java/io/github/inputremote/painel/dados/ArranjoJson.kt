package io.github.inputremote.painel.dados

import org.json.JSONArray
import org.json.JSONObject

/**
 * O [Arranjo] em texto, para guardar no aparelho. Um texto que não é arranjo — de uma versão
 * futura, ou estragado — vale como "não há arranjo", e a tela volta ao [Arranjo.PADRAO].
 */
object ArranjoJson {
    fun escrever(arranjo: Arranjo): String = JSONObject()
        .put("cards", JSONArray(arranjo.cards.map { JSONObject().put("chave", it.chave).put("destaque", it.destaque) }))
        .put("conhecidas", JSONArray(arranjo.conhecidas.sorted()))
        .toString()

    fun ler(texto: String?): Arranjo? = runCatching {
        val json = JSONObject(texto ?: return null)
        val cards = json.getJSONArray("cards").let { lista ->
            List(lista.length()) { i -> lista.getJSONObject(i) }
                .map { Arranjo.Card(it.getString("chave"), it.optBoolean("destaque")) }
        }
        val conhecidas = json.getJSONArray("conhecidas").let { lista -> List(lista.length()) { lista.getString(it) } }
        Arranjo(cards.distinctBy { it.chave }, conhecidas.toSet() + cards.map { it.chave })
    }.getOrNull()
}
