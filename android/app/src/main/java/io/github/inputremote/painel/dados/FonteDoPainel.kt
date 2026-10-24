package io.github.inputremote.painel.dados

/**
 * De onde o painel tira tudo: a procura na rede, os pareados e as ligações.
 *
 * A tela depende desta interface, e não do núcleo nativo: o [io.github.inputremote.painel.ui.Controlador]
 * é testado com uma fonte de mentira, e só o aplicativo de verdade carrega a biblioteca em Rust.
 */
interface FonteDoPainel {
    /** Pergunta à rede e devolve quem respondeu. Bloqueia por [duracaoMs]. */
    fun procurar(duracaoMs: Int): List<ComputadorAchado>

    /** Os computadores pareados, o mais recente primeiro. */
    fun pareados(): List<ComputadorPareado>

    /** Esquece o computador desta chave. */
    fun esquecer(chave: String)

    /** Começa a parear com um computador achado; nulo se não deu para começar. */
    fun parear(computador: ComputadorAchado): Ligacao?

    /** Volta ao computador pareado mais recente; nulo sem pareado. */
    fun voltar(): Ligacao?
}

/** Uma ligação com um computador. Segura para chamar de linhas diferentes. */
interface Ligacao {
    /** O próximo evento, esperando até [esperaMs]; nulo sem evento. Bloqueia. */
    fun proximo(esperaMs: Int): EventoDoNucleo?

    /** A resposta desta tela ao código. */
    fun confirmar(conferiu: Boolean)

    /** Começa ou para de receber o quadro. */
    fun assinar(quer: Boolean)

    /** O que a tela quer além do quadro. Vale até o próximo; uma ligação nova começa sem nenhum. */
    fun interessar(interesse: Interesse)

    /** Encerra a ligação. Depois disto, nada mais chega. */
    fun fechar()
}

/**
 * O que a tela quer além do quadro: os processos dos cards, e a lista de processos de qual
 * computador — a posição dele no quadro — está aberta. Sem nada disso, o computador não mede
 * processo nenhum.
 */
data class Interesse(val processos: List<String> = emptyList(), val lista: Int? = null)
