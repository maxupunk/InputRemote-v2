package io.github.inputremote.painel.dados

/**
 * Quais cards o painel mostra, em que ordem, e quais em destaque.
 *
 * Só conhece a [Bloco.chave] de cada métrica — que o núcleo mantém estável entre os segundos e
 * entre as versões —, e não a métrica em si. Por isso vale para qualquer bloco, inclusive os que
 * uma versão futura do computador mandar: uma chave nunca vista entra no fim, e uma que a pessoa
 * apagou continua conhecida, para poder voltar por "Adicionar card".
 *
 * Os cards de processo (`proc:docker`) são a exceção: quem os cria é a pessoa, por "Acompanhar".
 * Eles são o pedido ao computador — o que está no arranjo é o que ele mede ([processos]) —, então
 * não entram sozinhos e, tirados, são esquecidos.
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

    /**
     * Uma chave nova entra no fim, visível; uma já conhecida fica como a pessoa deixou. O card sob
     * demanda não entra assim ([ehSobDemanda]): o processo de outro painel ligado ao mesmo
     * computador, e cada sensor de temperatura, só aparecem quando a pessoa os põe.
     */
    fun conhecer(chaves: Collection<String>): Arranjo {
        val novas = chaves.filterNot { it in conhecidas || ehSobDemanda(it) }.distinct()
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

    /** Tira o card da tela. Ele continua conhecido — menos o sob demanda, que é esquecido. */
    fun remover(chave: String): Arranjo = copy(
        cards = cards.filterNot { it.chave == chave },
        conhecidas = if (ehSobDemanda(chave)) conhecidas - chave else conhecidas,
    )

    /**
     * O que "Adicionar card" oferece: o que existe agora ([rotulos], pela chave) e não está na
     * tela — os cards de sempre primeiro, depois os de cada sensor, cada grupo pelo nome.
     */
    fun disponiveis(rotulos: Map<String, String>): List<String> = rotulos.keys
        .filterNot(::visivel)
        .sortedWith(compareBy<String> { ehSensor(it) }.thenBy { rotulos[it] })

    /** Os nomes dos processos acompanhados, na ordem da tela. É o que se pede ao computador. */
    fun processos(): List<String> = cards.map { it.chave }.filter(::ehProcesso).map { it.removePrefix(PREFIXO_DE_PROCESSO) }

    /** Se cabe mais um processo: o computador mede no máximo [MAX_PROCESSOS]. */
    fun podeAcompanhar(): Boolean = processos().size < MAX_PROCESSOS

    /**
     * Um card para os processos com este nome — `docker` soma todos os que têm "docker" no nome.
     * Sem nome, já acompanhado ou sem lugar, nada muda.
     */
    fun acompanhar(nome: String): Arranjo {
        val chave = chaveDoProcesso(nome)
        if (chave == PREFIXO_DE_PROCESSO || visivel(chave) || !podeAcompanhar()) return this
        return adicionar(chave)
    }

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

        /** A faixa da cópia de arquivos, que só aparece enquanto há cópia. */
        const val COPIA = "copia"

        /** O começo da chave de um card de processo, a mesma do núcleo (`vista::PREFIXO_DE_PROCESSO`). */
        const val PREFIXO_DE_PROCESSO = "proc:"

        /** Quantos processos o computador mede por painel (`processos::MAX_VIGIADOS`). */
        const val MAX_PROCESSOS = 4

        /** A chave do card que acompanha este processo. */
        fun chaveDoProcesso(nome: String) = PREFIXO_DE_PROCESSO + nome.trim().lowercase()

        fun ehProcesso(chave: String) = chave.startsWith(PREFIXO_DE_PROCESSO)

        /** O começo da chave do card de um sensor de temperatura (`vista::PREFIXO_DE_SENSOR`). */
        const val PREFIXO_DE_SENSOR = "sensor:"

        fun ehSensor(chave: String) = chave.startsWith(PREFIXO_DE_SENSOR)

        /**
         * Um card que só existe porque a pessoa o pediu: um processo, um sensor. Não entra sozinho,
         * e tirado é esquecido — um computador com dezesseis núcleos não enche o painel por conta
         * própria.
         */
        fun ehSobDemanda(chave: String) = ehProcesso(chave) || ehSensor(chave)

        /**
         * O que se olha primeiro é o processador e a memória: grandes e no começo. Depois o que
         * mais muda (temperatura, rede), e por fim o que muda devagar.
         */
        val PADRAO: Arranjo = listOf(
            Card("cpu", destaque = true),
            Card("ram", destaque = true),
            Card("gpu"),
            Card("temp-cpu"),
            Card("rede"),
            Card("disco"),
            Card("bateria"),
            Card("temp-gpu"),
            Card("temp-disco"),
            Card("temp-placa"),
            Card("temp"),
            Card(ENLACE),
            Card(COPIA),
        ).let { cards -> Arranjo(cards, cards.map { it.chave }.toSet()) }
    }
}
