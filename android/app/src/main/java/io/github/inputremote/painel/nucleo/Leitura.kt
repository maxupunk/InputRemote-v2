package io.github.inputremote.painel.nucleo

import io.github.inputremote.painel.dados.Bloco
import io.github.inputremote.painel.dados.ComputadorAchado
import io.github.inputremote.painel.dados.ComputadorPareado
import io.github.inputremote.painel.dados.EventoDoNucleo
import io.github.inputremote.painel.dados.LinhaDeProcesso
import io.github.inputremote.painel.dados.Nivel
import io.github.inputremote.painel.dados.Vista
import io.github.inputremote.painel.dados.VistaDaMaquina
import io.github.inputremote.painel.dados.VistaDoEnlace
import org.json.JSONArray
import org.json.JSONObject

/**
 * O JSON do núcleo (`crates/ir-monitor-android/src/ponte.rs`) virando modelo.
 *
 * Tolerante de propósito: um campo que falta vira "sem dado", e um evento desconhecido é ignorado.
 * O aplicativo e o computador atualizam em momentos diferentes, e uma tela que quebra por um campo
 * novo é pior que uma que o ignora.
 */
internal object Leitura {
    /** Um evento, ou nulo para texto vazio ou um tipo que esta versão não conhece. */
    fun evento(texto: String): EventoDoNucleo? {
        if (texto.isBlank()) return null
        val json = JSONObject(texto)
        return when (json.optString("tipo")) {
            "codigo" -> EventoDoNucleo.Codigo(json.optString("digitos"))
            "aberta" -> EventoDoNucleo.Aberta(json.optString("computador"), json.optBoolean("pareou"))
            "quadro" -> EventoDoNucleo.Quadro(vista(json.optJSONObject("vista") ?: JSONObject()))
            "caiu" -> EventoDoNucleo.Caiu(json.optString("motivo"), json.optBoolean("recusado"))
            else -> null
        }
    }

    fun achados(texto: String): List<ComputadorAchado> = objetos(JSONArray(texto)).map {
        ComputadorAchado(it.optString("nome"), it.optString("endereco"))
    }

    fun pareados(texto: String): List<ComputadorPareado> = objetos(JSONArray(texto)).map {
        ComputadorPareado(it.optString("nome"), it.optString("chave"), it.optString("endereco"))
    }

    private fun vista(json: JSONObject) = Vista(
        maquinas = objetos(json.optJSONArray("maquinas")).map(::maquina),
        enlace = json.optJSONObject("enlace")?.let { enlace ->
            VistaDoEnlace(
                rotas = objetos(enlace.optJSONArray("rotas")).map(::bloco),
                vencendo = enlace.textoOuNulo("vencendo"),
            )
        },
        copia = json.optJSONObject("copia")?.let(::bloco),
    )

    private fun maquina(json: JSONObject) = VistaDaMaquina(
        nome = json.optString("nome"),
        aviso = json.textoOuNulo("aviso"),
        nivel = Nivel.de(json.optString("nivel")),
        blocos = objetos(json.optJSONArray("blocos")).map(::bloco),
        processos = json.optJSONArray("processos")?.let { lista -> objetos(lista).map(::linha) },
    )

    private fun linha(json: JSONObject) = LinhaDeProcesso(
        chave = json.optString("chave"),
        nome = json.optString("nome"),
        instancias = json.optInt("instancias"),
        cpu = json.optString("cpu", "—"),
        memoria = json.optString("memoria", "—"),
        cpuPermille = json.optInt("cpu_permille"),
        memoriaKib = json.optLong("memoria_kib"),
        nivel = Nivel.de(json.optString("nivel")),
    )

    private fun bloco(json: JSONObject) = Bloco(
        chave = json.optString("chave"),
        rotulo = json.optString("rotulo"),
        valor = json.optString("valor", "—"),
        detalhe = json.textoOuNulo("detalhe"),
        fracao = json.numeroOuNulo("fracao"),
        nivel = Nivel.de(json.optString("nivel", "sem_dado")),
        serie = json.numeroOuNulo("serie"),
    )

    private fun objetos(lista: JSONArray?): List<JSONObject> =
        if (lista == null) emptyList() else List(lista.length()) { lista.optJSONObject(it) }.filterNotNull()

    private fun JSONObject.textoOuNulo(campo: String): String? =
        if (isNull(campo)) null else optString(campo).takeIf { it.isNotEmpty() }

    private fun JSONObject.numeroOuNulo(campo: String): Float? =
        if (isNull(campo)) null else optDouble(campo).takeUnless { it.isNaN() }?.toFloat()
}
