package io.github.inputremote.painel.dados

/**
 * A ordem da lista de processos, parada por alguns segundos.
 *
 * Os números mudam a cada segundo, e reordenar a cada segundo faz a linha andar debaixo do dedo:
 * o toque em "Acompanhar" do Code caía no processo que subiu para o lugar dele (visto no tablet,
 * log 70). Então os valores mudam a cada segundo, mas as linhas só trocam de lugar a cada
 * [intervaloMs] — ou na hora, quando a pessoa escolhe outra coluna.
 */
class ListaEstavel(private val intervaloMs: Long = INTERVALO_MS) {
    private var chaves: List<String> = emptyList()
    private var ordenadaEm: Long? = null
    private var ordenadaPor: Ordem? = null

    /** As linhas deste segundo, na ordem que a tela deve mostrar. */
    fun arrumar(linhas: List<LinhaDeProcesso>, ordem: Ordem, agoraMs: Long): List<LinhaDeProcesso> {
        val desde = ordenadaEm
        if (ordem != ordenadaPor || desde == null || agoraMs - desde >= intervaloMs) {
            return ordem.ordenar(linhas).also {
                chaves = it.map(LinhaDeProcesso::chave)
                ordenadaEm = agoraMs
                ordenadaPor = ordem
            }
        }
        val porChave = linhas.associateBy { it.chave }
        val conhecidas = chaves.toSet()
        // Quem já estava fica no lugar; quem chegou agora entra no fim; quem sumiu sai.
        return chaves.mapNotNull { porChave[it] } + ordem.ordenar(linhas.filterNot { it.chave in conhecidas })
    }

    companion object {
        /** Tempo para ler a lista e tocar numa linha antes de ela mudar de lugar. */
        const val INTERVALO_MS = 5_000L
    }
}
