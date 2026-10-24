package io.github.inputremote.painel.dados

/**
 * O último minuto de cada número, para os mini-gráficos.
 *
 * Só na memória do aparelho: o computador não guarda nada disso, e o custo dele continua zero
 * (docs/12, §2). Cada série é chaveada pelo computador e pela [Bloco.chave], que é estável entre
 * os segundos.
 */
class Historico(private val tamanho: Int = PONTOS) {
    private val series = HashMap<String, ArrayDeque<Float>>()

    /** Anota os números deste quadro e devolve uma cópia das séries para a tela. */
    fun anotar(vista: Vista): Map<String, List<Float>> {
        for (maquina in vista.maquinas) {
            for (bloco in maquina.blocos) anotar(chave(maquina.nome, bloco.chave), bloco.serie)
        }
        vista.enlace?.rotas?.forEach { anotar(chave(ENLACE, it.chave), it.serie) }
        vista.copia?.let { anotar(chave(ENLACE, it.chave), it.serie) }
        return series.mapValues { it.value.toList() }
    }

    /** Esquece tudo: outro computador, outra história. */
    fun limpar() = series.clear()

    private fun anotar(chave: String, valor: Float?) {
        val serie = series.getOrPut(chave) { ArrayDeque(tamanho) }
        if (valor == null) {
            // Um buraco no número (a fonte parou de ler) começa a série de novo: ligar os pontos de
            // antes e de depois desenharia um valor que ninguém mediu.
            serie.clear()
            return
        }
        if (serie.size == tamanho) serie.removeFirst()
        serie.addLast(valor)
    }

    companion object {
        /** Um minuto, a uma amostra por segundo. */
        const val PONTOS = 60

        /** A "máquina" da faixa do enlace. */
        const val ENLACE = "enlace"

        /** A chave de uma série. */
        fun chave(maquina: String, bloco: String) = "$maquina/$bloco"
    }
}
