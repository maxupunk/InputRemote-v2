package io.github.inputremote.painel.dados

/**
 * Por qual coluna a lista de processos se ordena. O "i" do processador abre ordenada por ele, o da
 * memória por ela: a lista começa respondendo a pergunta que a pessoa fez.
 */
enum class Ordem(val nome: String) {
    PROCESSADOR("Processador"),
    MEMORIA("Memória"),
    ;

    /** As linhas, da mais pesada nesta coluna para a mais leve; empate pelo nome. */
    fun ordenar(linhas: List<LinhaDeProcesso>): List<LinhaDeProcesso> = when (this) {
        PROCESSADOR -> linhas.sortedWith(compareByDescending<LinhaDeProcesso> { it.cpuPermille }.thenBy { it.nome })
        MEMORIA -> linhas.sortedWith(compareByDescending<LinhaDeProcesso> { it.memoriaKib }.thenBy { it.nome })
    }

    /** Quanto da barra encher, de 0 a 1: o processador da máquina inteira, ou a memória do mais pesado. */
    fun fracao(linha: LinhaDeProcesso, linhas: List<LinhaDeProcesso>): Float = when (this) {
        PROCESSADOR -> linha.cpuPermille / 1000f
        MEMORIA -> linhas.maxOfOrNull { it.memoriaKib }?.takeIf { it > 0 }?.let { linha.memoriaKib.toFloat() / it } ?: 0f
    }

    /** O número desta coluna, já escrito. */
    fun valor(linha: LinhaDeProcesso): String = when (this) {
        PROCESSADOR -> linha.cpu
        MEMORIA -> linha.memoria
    }

    /** O da outra coluna, menor ao lado. */
    fun outro(linha: LinhaDeProcesso): String = when (this) {
        PROCESSADOR -> linha.memoria
        MEMORIA -> linha.cpu
    }

    companion object {
        /** A ordem que o "i" de um card abre: memória pela `ram`, processador pelo resto. */
        fun doCard(chave: String): Ordem = if (chave == "ram") MEMORIA else PROCESSADOR
    }
}
