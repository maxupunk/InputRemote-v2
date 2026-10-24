package io.github.inputremote.painel.nucleo

import io.github.inputremote.painel.dados.ComputadorAchado
import io.github.inputremote.painel.dados.ComputadorPareado
import io.github.inputremote.painel.dados.EventoDoNucleo
import io.github.inputremote.painel.dados.FonteDoPainel
import io.github.inputremote.painel.dados.Ligacao

/**
 * A fonte de verdade: o núcleo em Rust.
 *
 * @param pasta a pasta privada do aplicativo, onde o núcleo guarda a identidade e os pareados.
 * @param aparelho o nome deste aparelho, para a lista do computador.
 */
class FonteNativa(private val pasta: String, private val aparelho: String) : FonteDoPainel {
    override fun procurar(duracaoMs: Int): List<ComputadorAchado> =
        Leitura.achados(Nativo.procurar(duracaoMs))

    override fun pareados(): List<ComputadorPareado> = Leitura.pareados(Nativo.pareados(pasta))

    override fun esquecer(chave: String) = Nativo.esquecer(pasta, chave)

    override fun parear(computador: ComputadorAchado): Ligacao? =
        LigacaoNativa.de(Nativo.parear(pasta, aparelho, computador.nome, computador.endereco))

    override fun voltar(): Ligacao? = LigacaoNativa.de(Nativo.voltar(pasta, aparelho))
}

/** Uma ligação do núcleo, pelo número que o Rust devolveu. */
private class LigacaoNativa(private val id: Long) : Ligacao {
    override fun proximo(esperaMs: Int): EventoDoNucleo? = Leitura.evento(Nativo.proximo(id, esperaMs))

    override fun confirmar(conferiu: Boolean) = Nativo.confirmar(id, conferiu)

    override fun assinar(quer: Boolean) = Nativo.assinar(id, quer)

    override fun fechar() = Nativo.fechar(id)

    companion object {
        /** A ligação deste número; zero é "não deu". */
        fun de(id: Long): Ligacao? = if (id == 0L) null else LigacaoNativa(id)
    }
}
