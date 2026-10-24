package io.github.inputremote.painel.nucleo

/**
 * As funções do núcleo em Rust (`crates/ir-monitor-android/src/jni.rs`).
 *
 * Os nomes e as assinaturas são o contrato com o Rust: o JNI acha cada função pelo nome desta
 * classe e do método, e o `proguard-rules.pro` impede que o encurtamento os mude. Ninguém além de
 * [FonteNativa] chama isto.
 */
internal object Nativo {
    init {
        System.loadLibrary("ir_monitor_android")
    }

    /** Pergunta à rede por [duracaoMs] e devolve os computadores que responderam, em JSON. */
    @JvmStatic external fun procurar(duracaoMs: Int): String

    /** Os computadores pareados, em JSON, o mais recente primeiro. */
    @JvmStatic external fun pareados(pasta: String): String

    /** Esquece o computador desta chave. */
    @JvmStatic external fun esquecer(pasta: String, chave: String)

    /** Começa a parear com o computador deste IP. Devolve o número da ligação, ou 0. */
    @JvmStatic external fun parear(pasta: String, aparelho: String, computador: String, endereco: String): Long

    /** Volta ao computador pareado mais recente. Devolve o número da ligação, ou 0. */
    @JvmStatic external fun voltar(pasta: String, aparelho: String): Long

    /** O próximo evento da ligação em JSON, esperando até [esperaMs]; texto vazio sem evento. */
    @JvmStatic external fun proximo(id: Long, esperaMs: Int): String

    /** A resposta desta tela ao código. */
    @JvmStatic external fun confirmar(id: Long, conferiu: Boolean)

    /** Começa ou para de receber o quadro. */
    @JvmStatic external fun assinar(id: Long, quer: Boolean)

    /**
     * O que a tela quer além do quadro: os nomes dos processos acompanhados, em JSON, e de qual
     * computador (a posição no quadro) a lista de processos está aberta — ou -1.
     */
    @JvmStatic external fun interesse(id: Long, processos: String, lista: Int)

    /** Encerra a ligação. */
    @JvmStatic external fun fechar(id: Long)
}
