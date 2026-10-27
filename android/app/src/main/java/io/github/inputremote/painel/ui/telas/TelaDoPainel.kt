package io.github.inputremote.painel.ui.telas

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.inputremote.painel.dados.Arranjo
import io.github.inputremote.painel.dados.Bloco
import io.github.inputremote.painel.dados.Historico
import io.github.inputremote.painel.dados.Nivel
import io.github.inputremote.painel.dados.Ordem
import io.github.inputremote.painel.dados.PainelPersonalizado
import io.github.inputremote.painel.dados.Vista
import io.github.inputremote.painel.dados.VistaDaMaquina
import io.github.inputremote.painel.dados.VistaDoEnlace
import io.github.inputremote.painel.ui.Tela
import io.github.inputremote.painel.ui.tema.Cores
import io.github.inputremote.painel.ui.tema.cor
import io.github.inputremote.painel.ui.tema.numeros
import kotlinx.coroutines.launch

/**
 * A largura mínima de um card destacado: "100 %" a 40 sp cabe, e numa coluna de tablet deitado
 * (~380 dp no de 8") o processador e a memória ficam lado a lado.
 */
private val LARGURA_DO_DESTAQUE = 170.dp

/** A largura mínima de um card compacto. */
private val LARGURA_DO_COMPACTO = 140.dp

/** O nome da faixa do enlace, quando ela é oferecida em "Adicionar card". */
private const val NOME_DO_ENLACE = "Entre os computadores"

/** O nome da faixa da cópia, idem. */
private const val NOME_DA_COPIA = "Cópia de arquivos"

/** Os cards cujo "i" abre a lista de processos. */
private val COM_LISTA = setOf("cpu", "ram")

/**
 * O painel multitelas com suporte a deslize (swipe) e abas superiores:
 * - Página 0: Painel geral das máquinas conectadas, com cards alinhados e mesma altura.
 * - Página 1: Processos ao vivo com pesquisa, filtros e atalhos rápidos.
 * - Página 2: Conexões e diagnóstico (latências Bluetooth e Wi-Fi com gráficos, cópia de arquivos).
 * - Página 3: Ajustes do visor e pareamento.
 */
@Composable
fun TelaDoPainel(painel: Tela.Painel, acoes: Acoes, salvo: Aparelho) {
    var editando by rememberSaveable { mutableStateOf(false) }
    // Na edição, cada mudança guarda o arranjo de antes, para o "Desfazer"; ao concluir, esquece.
    val desfazer = remember { Desfazer() }
    LaunchedEffect(editando) { if (!editando) desfazer.esquecer() }
    val aparelho = if (editando) desfazer.guardando(salvo) else salvo
    var ordem by rememberSaveable { mutableStateOf(Ordem.PROCESSADOR) }
    var maquinaProcessos by rememberSaveable { mutableIntStateOf(0) }
    val escopo = rememberCoroutineScope()

    val totalPaineis = aparelho.paineis.size
    val titulos = remember(aparelho.paineis) { aparelho.paineis.map { it.nome } + PaginasFixas.TODAS }
    val indiceProcessos = totalPaineis
    val indiceConexoes = totalPaineis + 1
    val indiceAjustes = totalPaineis + 2

    val pagerState = rememberPagerState(initialPage = 0, pageCount = { titulos.size })

    // Se o usuário deslizar entre as abas de painéis, sincroniza o painel ativo
    LaunchedEffect(pagerState.currentPage, totalPaineis) {
        if (pagerState.currentPage < totalPaineis) {
            val p = aparelho.paineis[pagerState.currentPage]
            if (aparelho.painelAtivoId != p.id) {
                aparelho.aoSelecionarPainel(p.id)
            }
        }
    }

    val informar: ((Int, String) -> Unit)? = if (aparelho.editavel) {
        { maquina, chave ->
            ordem = Ordem.doCard(chave)
            maquinaProcessos = maquina
            acoes.abrirLista(maquina)
            escopo.launch { pagerState.animateScrollToPage(indiceProcessos) }
        }
    } else {
        null
    }

    // Assina processos quando a tela de processos estiver visível
    LaunchedEffect(pagerState.currentPage, maquinaProcessos, indiceProcessos) {
        if (pagerState.currentPage == indiceProcessos) {
            acoes.abrirLista(maquinaProcessos)
        } else if (painel.lista != null) {
            acoes.abrirLista(null)
        }
    }

    val rotulos = remember(painel.vista) { rotulosDa(painel.vista) }
    LaunchedEffect(rotulos.keys) { aparelho.editarArranjo { it.conhecer(rotulos.keys) } }

    BoxWithConstraints(Modifier.fillMaxSize().padding(horizontal = 8.dp, vertical = 4.dp)) {
        val deitado = maxWidth > maxHeight
        val painelAtual = if (pagerState.currentPage < totalPaineis) {
            aparelho.paineis[pagerState.currentPage]
        } else {
            aparelho.painelAtivo
        }
        val enlace = painel.vista.enlace?.takeIf { painelAtual.arranjo.visivel(Arranjo.ENLACE) }
        val copia = painelAtual.arranjo.visivel(Arranjo.COPIA) && (painel.vista.copia != null || editando)

        val faixas: @Composable () -> Unit = {
            if (deitado) {
                Row(
                    modifier = Modifier.fillMaxWidth().height(IntrinsicSize.Max),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (enlace != null) {
                        Box(Modifier.weight(1f).fillMaxHeight()) {
                            FaixaDoEnlace(enlace, painel.series, editando, aparelho, Modifier.fillMaxHeight())
                        }
                    }
                    if (copia) {
                        Box(Modifier.weight(1f).fillMaxHeight()) {
                            FaixaDaCopia(painel.vista.copia, painel.series, editando, aparelho, Modifier.fillMaxHeight())
                        }
                    } else if (enlace != null) {
                        Spacer(Modifier.weight(1f).fillMaxHeight())
                    }
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (enlace != null) FaixaDoEnlace(enlace, painel.series, editando, aparelho, Modifier.fillMaxWidth())
                    if (copia) FaixaDaCopia(painel.vista.copia, painel.series, editando, aparelho, Modifier.fillMaxWidth())
                }
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Cabecalho(
                painel = painel,
                acoes = acoes,
                aparelho = aparelho,
                titulos = titulos,
                paginaAtual = pagerState.currentPage,
                aoMudarPagina = { pagina -> escopo.launch { pagerState.animateScrollToPage(pagina) } },
                editar = {
                    editando = true
                    escopo.launch { pagerState.animateScrollToPage(pagerState.currentPage.coerceAtMost(totalPaineis - 1)) }
                },
            )

            if (editando && pagerState.currentPage < totalPaineis) {
                val aoInverter = if (painel.vista.maquinas.size > 1) {
                    {
                        val invertido = painelAtual.inverterLados(painel.vista.maquinas)
                        aparelho.aoTrocarLadoMaquinas(painelAtual.id, invertido.ordemMaquinas)
                    }
                } else null
                BarraDeEdicao(
                    aparelho = aparelho,
                    rotulos = rotulos,
                    aoInverterLados = aoInverter,
                    desfazer = desfazer,
                    concluir = { editando = false },
                )
            }

            if (!aparelho.editavel) {
                // Modo passivo (ex: protetor de tela Dream)
                VisaoGeralDoPainel(
                    painel = painel,
                    aparelho = aparelho,
                    painelPersonalizado = painelAtual,
                    editando = false,
                    deitado = deitado,
                    faixas = faixas,
                    informar = null,
                    modifier = Modifier.weight(1f),
                )
            } else {
                // Modo interativo com pager horizontal deslizável
                HorizontalPager(
                    state = pagerState,
                    key = { pagina ->
                        if (pagina < totalPaineis) {
                            val p = aparelho.paineis[pagina]
                            "${p.id}_${p.nome}_${p.ordemMaquinas}"
                        } else {
                            titulos.getOrElse(pagina) { "$pagina" }
                        }
                    },
                    modifier = Modifier.weight(1f).fillMaxWidth(),
                    userScrollEnabled = !editando,
                ) { pagina ->
                    when {
                        pagina < totalPaineis -> {
                            val painelDaPagina = aparelho.paineis[pagina]
                            VisaoGeralDoPainel(
                                painel = painel,
                                aparelho = aparelho,
                                painelPersonalizado = painelDaPagina,
                                editando = editando,
                                deitado = deitado,
                                faixas = faixas,
                                informar = informar,
                                modifier = Modifier.fillMaxSize(),
                            )
                        }
                        pagina == indiceProcessos -> {
                            TelaDeProcessos(
                                maquinas = painel.vista.maquinas,
                                maquinaIndex = maquinaProcessos,
                                aoMudarMaquina = {
                                    maquinaProcessos = it
                                    acoes.abrirLista(it)
                                },
                                ordemInicial = ordem,
                                aparelho = aparelho,
                                aoPesquisar = acoes.pesquisarProcesso,
                                modifier = Modifier.fillMaxSize(),
                            )
                        }
                        pagina == indiceConexoes -> {
                            TelaDeConexoes(
                                vista = painel.vista,
                                series = painel.series,
                                modifier = Modifier.fillMaxSize(),
                            )
                        }
                        pagina == indiceAjustes -> {
                            TelaDeAjustes(
                                painel = painel,
                                acoes = acoes,
                                aparelho = aparelho,
                                aoIniciarEdicao = {
                                    editando = true
                                    escopo.launch { pagerState.animateScrollToPage(0) }
                                },
                                modifier = Modifier.fillMaxSize(),
                            )
                        }
                    }
                }
            }
        }
    }
}

/** Visão geral do painel: máquinas conectadas lado a lado e faixas de enlace/cópia. */
@Composable
private fun VisaoGeralDoPainel(
    painel: Tela.Painel,
    aparelho: Aparelho,
    painelPersonalizado: PainelPersonalizado,
    editando: Boolean,
    deitado: Boolean,
    faixas: @Composable () -> Unit,
    informar: ((Int, String) -> Unit)?,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.alpha(if (painel.segundosSemDados > 0) 0.45f else 1f),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (!deitado) faixas()
        Maquinas(
            painel = painel,
            aparelho = aparelho,
            painelPersonalizado = painelPersonalizado,
            editando = editando,
            lado = deitado,
            modifier = Modifier.weight(1f),
            informar = informar,
        )
        if (deitado) faixas()
    }
}

/**
 * O nome de cada card que existe agora, pela chave. A cópia está sempre: ela só aparece quando há
 * cópia, mas a pessoa precisa poder pô-la e tirá-la a qualquer hora.
 */
private fun rotulosDa(vista: Vista): Map<String, String> {
    val blocos = vista.maquinas.flatMap { it.blocos }.associate { it.chave to it.rotulo } + (Arranjo.COPIA to NOME_DA_COPIA)
    return if (vista.enlace == null) blocos else blocos + (Arranjo.ENLACE to NOME_DO_ENLACE)
}

@Composable
private fun Cabecalho(
    painel: Tela.Painel,
    acoes: Acoes,
    aparelho: Aparelho,
    titulos: List<String>,
    paginaAtual: Int,
    aoMudarPagina: (Int) -> Unit,
    editar: () -> Unit,
) {
    val velho = painel.segundosSemDados > 0
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Ponto(if (velho) Cores.atencao else Cores.bom)
        Spacer(Modifier.width(8.dp))
        Text(
            if (velho) "${painel.computador} · sem dados há ${painel.segundosSemDados} s" else painel.computador,
            color = if (velho) Cores.atencao else Cores.textoFraco,
            fontSize = 13.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )

        if (aparelho.editavel) {
            BarraDeNavegacao(
                titulos = titulos,
                paginaAtual = paginaAtual,
                aoMudarPagina = aoMudarPagina,
                modifier = Modifier.padding(horizontal = 8.dp),
            )
            Menu(acoes, aparelho, editar)
        }
    }
}

@Composable
private fun Menu(acoes: Acoes, aparelho: Aparelho, editar: () -> Unit) {
    var aberto by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { aberto = true }) { Text("⋮", fontSize = 20.sp, color = Cores.textoFraco) }
        DropdownMenu(expanded = aberto, onDismissRequest = { aberto = false }) {
            val item: @Composable (String, () -> Unit) -> Unit = { texto, acao ->
                DropdownMenuItem(text = { Text(texto) }, onClick = { aberto = false; acao() })
            }
            item("Editar painel", editar)
            item(if (aparelho.sempreDeitado) "Girar com o aparelho" else "Sempre deitado", aparelho.alternarDeitado)
            item(if (aparelho.escurecido) "Tela no brilho normal" else "Escurecer a tela", aparelho.alternarEscurecer)
            item("Parear outro computador", acoes.procurar)
            item("Esquecer este computador", acoes.esquecer)
        }
    }
}

@Composable
private fun Maquinas(
    painel: Tela.Painel,
    aparelho: Aparelho,
    painelPersonalizado: PainelPersonalizado,
    editando: Boolean,
    lado: Boolean,
    modifier: Modifier,
    informar: ((Int, String) -> Unit)?,
) {
    val maquinas = painelPersonalizado.ordenarMaquinas(painel.vista.maquinas)
    val deCada = { posicao: Int -> informar?.let { abrir -> { chave: String -> abrir(posicao, chave) } } }
    val trocarLados = if (painel.vista.maquinas.size > 1) {
        {
            val invertido = painelPersonalizado.inverterLados(painel.vista.maquinas)
            aparelho.aoTrocarLadoMaquinas(painelPersonalizado.id, invertido.ordemMaquinas)
        }
    } else null

    if (lado && maquinas.size > 1) {
        Row(modifier, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            maquinas.forEach { maquina ->
                key(maquina.nome) {
                    val mod = Modifier.weight(1f).fillMaxHeight()
                    val originalIndex = painel.vista.maquinas.indexOfFirst { it.nome == maquina.nome }.coerceAtLeast(0)
                    CartaoDaMaquina(
                        maquina = maquina,
                        series = painel.series,
                        aparelho = aparelho,
                        painelPersonalizado = painelPersonalizado,
                        editando = editando,
                        modifier = mod,
                        rolavel = true,
                        informar = deCada(originalIndex),
                        aoTrocarLados = trocarLados,
                    )
                }
            }
        }
    } else {
        Column(modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            maquinas.forEach { maquina ->
                key(maquina.nome) {
                    val originalIndex = painel.vista.maquinas.indexOfFirst { it.nome == maquina.nome }.coerceAtLeast(0)
                    CartaoDaMaquina(
                        maquina = maquina,
                        series = painel.series,
                        aparelho = aparelho,
                        painelPersonalizado = painelPersonalizado,
                        editando = editando,
                        modifier = Modifier.fillMaxWidth(),
                        informar = deCada(originalIndex),
                        aoTrocarLados = trocarLados,
                    )
                }
            }
        }
    }
}

/**
 * Um computador: o nome no topo (fixo, nunca cortado), os cards destacados grandes e o resto
 * compacto. Se não couber em altura, rola apenas o interior dos cards.
 */
@Composable
private fun CartaoDaMaquina(
    maquina: VistaDaMaquina,
    series: Map<String, List<Float>>,
    aparelho: Aparelho,
    painelPersonalizado: PainelPersonalizado,
    editando: Boolean,
    modifier: Modifier,
    rolavel: Boolean = false,
    informar: ((String) -> Unit)? = null,
    aoTrocarLados: (() -> Unit)? = null,
) {
    val borda = if (maquina.nivel == Nivel.ATENCAO || maquina.nivel == Nivel.CRITICO) maquina.nivel.cor() else Cores.borda
    val (grandes, compactos) = painelPersonalizado.arranjo.ordenar(maquina.blocos) { it.chave }.partition { it.second.destaque }
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = Cores.superficie),
        border = BorderStroke(1.dp, borda),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 6.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            // O cabeçalho fica fora da rolagem para que o nome nunca seja cortado no topo
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    maquina.nome,
                    fontSize = 15.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = Cores.texto,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (editando && aoTrocarLados != null) {
                    TextButton(
                        onClick = aoTrocarLados,
                        modifier = Modifier.padding(end = 4.dp),
                    ) {
                        Text("⇄ Inverter lado", fontSize = 12.sp, color = Cores.acao)
                    }
                }
                maquina.aviso?.let {
                    Text(it, color = Cores.textoFraco, fontSize = 12.sp, maxLines = 1)
                }
            }

            val rolagem = if (rolavel) Modifier.verticalScroll(rememberScrollState()) else Modifier
            Column(
                modifier = rolagem,
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                val desenhar: @Composable (Bloco, Boolean, Modifier) -> Unit = { bloco, destaque, mod ->
                    Editavel(bloco.chave, editando, aparelho, mod) {
                        val serie = series[Historico.chave(maquina.nome, bloco.chave)].orEmpty()
                        val abrir = informar?.takeIf { !editando && bloco.chave in COM_LISTA }
                        BlocoDeMetrica(bloco, serie, destaque, Modifier.fillMaxWidth(), abrir?.let { { it(bloco.chave) } })
                    }
                }
                Grade(grandes.map { it.first }, LARGURA_DO_DESTAQUE) { bloco, mod -> desenhar(bloco, true, mod) }
                Grade(compactos.map { it.first }, LARGURA_DO_COMPACTO) { bloco, mod -> desenhar(bloco, false, mod) }
            }
        }
    }
}

/** A faixa do enlace: fina, numa linha — a volta de cada rota e qual está ganhando. */
@Composable
fun FaixaDoEnlace(
    enlace: VistaDoEnlace,
    series: Map<String, List<Float>>,
    editando: Boolean,
    aparelho: Aparelho,
    modifier: Modifier = Modifier,
) {
    Editavel(Arranjo.ENLACE, editando, aparelho, podeMover = false, podeDestacar = false, modifier = modifier) {
        Card(
            modifier = Modifier.fillMaxSize(),
            colors = CardDefaults.cardColors(containerColor = Cores.superficie),
            border = BorderStroke(1.dp, Cores.borda),
        ) {
            Row(
                Modifier.fillMaxSize().padding(horizontal = 8.dp, vertical = 6.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                enlace.rotas.forEach { bloco ->
                    val serie = series[Historico.chave(Historico.ENLACE, bloco.chave)].orEmpty()
                    BlocoDeMetrica(bloco, serie, destaque = false, Modifier.weight(1f).fillMaxHeight())
                }

                // Card da informação de conexão: compacto, direto e com estilo e dimensões idênticos aos cards de rota
                CardMaisRapida(
                    vencendo = enlace.vencendo,
                    modifier = Modifier.weight(1f).fillMaxHeight(),
                )
            }
        }
    }
}

/** Card compacto indicando a melhor rota de conexão atual, com altura e alinhamento idênticos aos cards de rota. */
@Composable
private fun CardMaisRapida(
    vencendo: String?,
    modifier: Modifier = Modifier,
) {
    val rotuloVencendo = vencendo?.replaceFirstChar { it.uppercase() } ?: "Medindo…"
    Column(
        modifier = modifier
            .clip(RoundedCornerShape(12.dp))
            .background(Cores.fundo)
            .padding(horizontal = 7.dp, vertical = 4.dp),
        verticalArrangement = Arrangement.spacedBy(1.dp),
    ) {
        Text(
            text = "Mais rápida",
            color = Cores.textoFraco,
            fontSize = 12.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.Bottom,
        ) {
            Text(
                text = rotuloVencendo,
                style = numeros,
                color = if (vencendo != null) Cores.bom else Cores.textoFraco,
                fontSize = 18.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Box(modifier = Modifier.fillMaxWidth().height(3.dp))
    }
}

/**
 * A cópia de arquivos entre os computadores, numa faixa com alinhamento e mesma altura do enlace.
 */
@Composable
fun FaixaDaCopia(
    copia: Bloco?,
    series: Map<String, List<Float>>,
    editando: Boolean,
    aparelho: Aparelho,
    modifier: Modifier = Modifier,
) {
    Editavel(Arranjo.COPIA, editando, aparelho, podeMover = false, podeDestacar = false, modifier = modifier) {
        Card(
            modifier = Modifier.fillMaxSize(),
            colors = CardDefaults.cardColors(containerColor = Cores.superficie),
            border = BorderStroke(1.dp, Cores.borda),
        ) {
            if (copia == null) {
                Box(
                    modifier = Modifier.fillMaxSize().padding(horizontal = 10.dp, vertical = 6.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    Text(
                        "$NOME_DA_COPIA: aparece aqui enquanto uma cópia acontece",
                        color = Cores.textoFraco,
                        fontSize = 12.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            } else {
                val serie = series[Historico.chave(Historico.ENLACE, copia.chave)].orEmpty()
                Box(
                    modifier = Modifier.fillMaxSize().padding(horizontal = 8.dp, vertical = 6.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    BlocoDeMetrica(copia, serie, destaque = false, Modifier.fillMaxSize())
                }
            }
        }
    }
}

/** Cards em linhas, quantos couberem na largura, todos da mesma altura matemática. */
@Composable
private fun Grade(blocos: List<Bloco>, largura: Dp, item: @Composable (Bloco, Modifier) -> Unit) {
    if (blocos.isEmpty()) return
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val porLinha = (maxWidth / largura).toInt().coerceIn(1, 4)
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            blocos.chunked(porLinha).forEach { linha ->
                Row(
                    modifier = Modifier.fillMaxWidth().height(IntrinsicSize.Max),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    linha.forEach { item(it, Modifier.weight(1f).fillMaxHeight()) }
                    repeat(porLinha - linha.size) { Spacer(Modifier.weight(1f).fillMaxHeight()) }
                }
            }
        }
    }
}
