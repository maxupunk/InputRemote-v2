package io.github.inputremote.painel.dados

import org.json.JSONArray
import org.json.JSONObject

/**
 * Um painel configurado pelo usuário: identificador único, nome exibido na aba,
 * os cards de cada computador e ordem personalizada das máquinas.
 *
 * Cada computador tem os seus cards ([porMaquina], pelo nome): adicionar, tirar ou mover num não mexe
 * no outro. O [arranjo] do painel guarda as faixas de baixo (enlace e cópia), que são dos dois, e é o
 * ponto de partida de um computador que ainda não foi arrumado — o primeiro toque nele copia e muda
 * a cópia.
 *
 * Imutável: cada alteração devolve uma nova instância.
 */
data class PainelPersonalizado(
    val id: String,
    val nome: String,
    val arranjo: Arranjo = Arranjo.PADRAO,
    val ordemMaquinas: List<String> = emptyList(),
    val porMaquina: Map<String, Arranjo> = emptyMap(),
) {
    /** Os cards deste computador; sem computador ([maquina] nulo), os do painel — as faixas. */
    fun arranjoDe(maquina: String?): Arranjo =
        maquina?.let { porMaquina[normalizar(it)] } ?: arranjo

    /** Muda os cards de um computador, ou os do painel com [maquina] nulo. */
    fun editar(maquina: String?, edicao: (Arranjo) -> Arranjo): PainelPersonalizado {
        val antes = arranjoDe(maquina)
        val depois = edicao(antes)
        return when {
            depois == antes -> this
            maquina == null -> copy(arranjo = depois)
            else -> copy(porMaquina = porMaquina + (normalizar(maquina) to depois))
        }
    }

    /**
     * Cada chave nova entra onde ela aparece: no computador que a mandou, se ele já foi arrumado, e no
     * painel, que é o ponto de partida dos outros e onde moram as faixas.
     */
    fun conhecer(chaves: Map<String, Collection<String>>, doPainel: Collection<String>): PainelPersonalizado {
        val todas = chaves.values.flatten() + doPainel
        val maquinas = porMaquina.mapValues { (nome, arranjo) ->
            arranjo.conhecer(chaves.entries.firstOrNull { normalizar(it.key) == nome }?.value.orEmpty())
        }
        val novo = copy(arranjo = arranjo.conhecer(todas), porMaquina = maquinas)
        return if (novo == this) this else novo
    }

    /** Todos os cards como vieram de fábrica, nos dois computadores. */
    fun restaurado(): PainelPersonalizado =
        copy(arranjo = Arranjo.PADRAO.conhecer(arranjo.conhecidas), porMaquina = emptyMap())

    /** Os processos acompanhados em qualquer computador: é o que se pede a eles. */
    fun processos(): List<String> = (listOf(arranjo) + porMaquina.values).flatMap { it.processos() }.distinct()

    /**
     * Retorna as máquinas na ordem definida para este painel.
     * Se não houver ordem gravada ou menos de 2 máquinas, mantém a ordem original recebida.
     */
    fun ordenarMaquinas(maquinas: List<VistaDaMaquina>): List<VistaDaMaquina> {
        if (ordemMaquinas.isEmpty() || maquinas.size < 2) return maquinas
        val ordemIndex = ordemMaquinas.withIndex().associate { normalizar(it.value) to it.index }
        return maquinas.sortedBy { ordemIndex[normalizar(it.nome)] ?: Int.MAX_VALUE }
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
        /** O nome de um computador como chave: sem espaço nas pontas e sem maiúscula. */
        private fun normalizar(nome: String) = nome.trim().lowercase()

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
                    .put("porMaquina", JSONObject(painel.porMaquina.mapValues { JSONObject(ArranjoJson.escrever(it.value)) }))
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
                val maquinas = obj.optJSONObject("porMaquina")?.let { mapa ->
                    mapa.keys().asSequence().mapNotNull { chave ->
                        ArranjoJson.ler(mapa.optJSONObject(chave)?.toString())?.let { chave to it }
                    }.toMap()
                }.orEmpty()
                PainelPersonalizado(id, nome, arranjo, ordem, maquinas)
            }
        }.getOrNull()
    }
}

