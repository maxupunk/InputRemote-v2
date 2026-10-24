package io.github.inputremote.painel.dados

/**
 * Quais cards o painel mostra, em que ordem, e quais em destaque.
 *
 * Só conhece a [Bloco.chave] de cada métrica — que o núcleo mantém estável entre os segundos e
 * entre as versões —, e não a métrica em si. Por isso vale para qualquer bloco, inclusive os que
 * uma versão futura do computador mandar: uma chave nunca vista entra no fim, e uma que a pessoa
 * apagou continua conhecida, para poder voltar por "Adicionar card".
 *
 * Imutável: toda edição devolve um arranjo novo.
 */
data class Arranjo(
    /** Os cards visíveis, na ordem da tela. */
    val cards: List<Card>,
    /** Toda chave que este aparelho já viu, visível ou não. */
    val conhecidas: Set<String>,
) {
    /** Um card: a métrica, e se ela aparece grande. */
    data class Card(val chave: String, val destaque: Boolean = false)

    /** Se a chave está na tela. */
    fun visivel(chave: String): Boolean = cards.any { it.chave == chave }

    /** As chaves conhecidas que a pessoa tirou da tela. */
    fun ocultas(): Set<String> = conhecidas - cards.map { it.chave }.toSet()

    /** Uma chave nova entra no fim, visível; uma já conhecida fica como a pessoa deixou. */
    fun conhecer(chaves: Collection<String>): Arranjo {
        val novas = chaves.filterNot { it in conhecidas }.distinct()
        if (novas.isEmpty()) return this
        return copy(cards = cards + novas.map { Card(it) }, conhecidas = conhecidas + novas)
    }

    /** Anda o card [passo] posições; nos extremos, para. */
    fun mover(chave: String, passo: Int): Arranjo {
        val de = cards.indexOfFirst { it.chave == chave }
        if (de < 0) return this
        val para = (de + passo).coerceIn(0, cards.lastIndex)
        if (para == de) return this
        val lista = cards.toMutableList()
        lista.add(para, lista.removeAt(de))
        return copy(cards = lista)
    }

    /** Tira o card da tela. Ele continua conhecido. */
    fun remover(chave: String): Arranjo = copy(cards = cards.filterNot { it.chave == chave })

    /** Põe de volta, no fim. */
    fun adicionar(chave: String): Arranjo =
        if (visivel(chave)) this else copy(cards = cards + Card(chave), conhecidas = conhecidas + chave)

    /** Grande vira normal, normal vira grande. */
    fun alternarDestaque(chave: String): Arranjo =
        copy(cards = cards.map { if (it.chave == chave) it.copy(destaque = !it.destaque) else it })

    /**
     * Os itens que estão na tela, na ordem do arranjo, cada um com o seu card. O que não está no
     * arranjo fica de fora — inclusive uma chave nova, até [conhecer] a pôr nele.
     */
    fun <T> ordenar(itens: List<T>, chaveDe: (T) -> String): List<Pair<T, Card>> {
        val porChave = itens.associateBy(chaveDe)
        return cards.mapNotNull { card -> porChave[card.chave]?.let { it to card } }
    }

    companion object {
        /** A faixa "Entre os computadores", que também é um card que se tira e se põe. */
        const val ENLACE = "enlace"

        /**
         * O que se olha primeiro é o processador e a memória: grandes e no começo. Depois o que
         * mais muda (temperatura, rede), e por fim o que muda devagar.
         */
        val PADRAO: Arranjo = listOf(
            Card("cpu", destaque = true),
            Card("ram", destaque = true),
            Card("temp-cpu"),
            Card("rede"),
            Card("disco"),
            Card("bateria"),
            Card("temp-gpu"),
            Card("temp-disco"),
            Card("temp-placa"),
            Card("temp"),
            Card(ENLACE),
        ).let { cards -> Arranjo(cards, cards.map { it.chave }.toSet()) }
    }
}
