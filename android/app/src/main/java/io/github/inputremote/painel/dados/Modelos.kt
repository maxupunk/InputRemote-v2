package io.github.inputremote.painel.dados

/**
 * O que o painel desenha, como o Rust o montou (`ir_monitor::vista`).
 *
 * O aplicativo não conhece métrica nenhuma: cada número é um [Bloco] genérico, com o texto pronto,
 * quanto da barra encher, o nível e o valor do mini-gráfico. Uma métrica nova no Rust aparece aqui
 * sem uma linha de Kotlin a mais.
 */
data class Vista(
    val maquinas: List<VistaDaMaquina>,
    val enlace: VistaDoEnlace?,
    /** A cópia de arquivos entre os computadores, em curso ou que acabou há pouco. */
    val copia: Bloco? = null,
)

/** O cartão de um computador. */
data class VistaDaMaquina(
    val nome: String,
    /** Uma frase no lugar dos blocos: aguardando, desatualizado, desconectado. */
    val aviso: String?,
    /** O pior nível entre os blocos, para a borda do cartão. */
    val nivel: Nivel,
    /** Os cards, inclusive os dos processos acompanhados (`proc:docker`). */
    val blocos: List<Bloco>,
    /** A lista de processos, quando pedida e já medida; nula antes disso. */
    val processos: List<LinhaDeProcesso>? = null,
)

/** Uma linha da lista de processos: os de mesmo nome somados. */
data class LinhaDeProcesso(
    /** A chave do card que acompanharia este processo, pronta. */
    val chave: String,
    val nome: String,
    /** Quantos processos com este nome. */
    val instancias: Int,
    /** "12,3 %". */
    val cpu: String,
    /** "1,2 GB". */
    val memoria: String,
    /** Para ordenar pelo processador: milésimos da máquina. */
    val cpuPermille: Int,
    /** Para ordenar pela memória. */
    val memoriaKib: Long,
    val nivel: Nivel,
)

/** A faixa do enlace entre os dois computadores. */
data class VistaDoEnlace(
    val rotas: List<Bloco>,
    /** "rede" ou "Bluetooth", quando as duas respondem. */
    val vencendo: String?,
)

/** Um número na tela. */
data class Bloco(
    /** Estável entre os segundos: o histórico do mini-gráfico é guardado por ela. */
    val chave: String,
    val rotulo: String,
    val valor: String,
    val detalhe: String?,
    /** Quanto da barra encher, de 0 a 1; sem barra quando nulo. */
    val fracao: Float?,
    val nivel: Nivel,
    /** O número do mini-gráfico deste segundo; sem gráfico quando nulo. */
    val serie: Float?,
)

/** Quando um número fica âmbar ou vermelho. Quem decide é o Rust (`ir_monitor::nivel`). */
enum class Nivel {
    NORMAL,
    ATENCAO,
    CRITICO,
    SEM_DADO,
    ;

    companion object {
        /** O nível pelo nome que vem no JSON. Um nome desconhecido vale como normal. */
        fun de(nome: String): Nivel = when (nome) {
            "atencao" -> ATENCAO
            "critico" -> CRITICO
            "sem_dado" -> SEM_DADO
            else -> NORMAL
        }
    }
}

/** Um computador que respondeu à procura na rede. */
data class ComputadorAchado(val nome: String, val endereco: String)

/** Um computador pareado com este aparelho. */
data class ComputadorPareado(val nome: String, val chave: String, val endereco: String)

/** O que a ligação com o computador conta. */
sealed interface EventoDoNucleo {
    /** Pareando: os seis dígitos a comparar. */
    data class Codigo(val digitos: String) : EventoDoNucleo

    /** A ligação abriu. */
    data class Aberta(val computador: String, val pareou: Boolean) : EventoDoNucleo

    /** O quadro deste segundo. */
    data class Quadro(val vista: Vista) : EventoDoNucleo

    /** A ligação acabou; [recusado] quando o computador não aceita mais este aparelho. */
    data class Caiu(val motivo: String, val recusado: Boolean) : EventoDoNucleo
}
