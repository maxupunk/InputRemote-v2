package io.github.inputremote.painel.ui

import io.github.inputremote.painel.dados.ComputadorAchado
import io.github.inputremote.painel.dados.Vista

/** O que a tela mostra agora. Um estado de cada vez, e a tela só o desenha. */
sealed interface Tela {
    /** Abrindo: ainda não se sabe se há computador pareado. */
    data object Abrindo : Tela

    /** Sem computador: procurando um na rede para parear. */
    data class Procurando(
        val achados: List<ComputadorAchado> = emptyList(),
        val procurando: Boolean = false,
        /** Por que se está aqui, quando não foi escolha: um pareamento recusado, por exemplo. */
        val recado: String? = null,
    ) : Tela

    /** Pareando: o código nas duas telas. */
    data class Conferindo(
        val computador: String,
        /** Os seis dígitos; nulo enquanto o computador não chega ao código. */
        val digitos: String? = null,
        /** Depois de "São iguais": esperando a outra tela confirmar também. */
        val aguardandoOutraTela: Boolean = false,
    ) : Tela

    /** Ligando — ou religando depois de uma queda. */
    data class Conectando(
        val computador: String,
        /** Por que caiu da última vez, quando caiu. */
        val motivo: String? = null,
    ) : Tela

    /** O painel. */
    data class Painel(
        val computador: String,
        val vista: Vista,
        /** O último minuto de cada número, chaveado por `Historico.chave`. */
        val series: Map<String, List<Float>>,
        /** Há quantos segundos não chega quadro; zero com o quadro em dia. */
        val segundosSemDados: Int = 0,
    ) : Tela
}
