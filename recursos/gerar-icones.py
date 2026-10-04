#!/usr/bin/env python3
"""Desenha o ícone do InputRemote e gera todos os tamanhos.

    python recursos/gerar-icones.py

Este script **é** o ícone. Não existe um `.svg` ao lado dele por decisão deliberada: dois arquivos
com o mesmo desenho sempre acabam diferentes, e aí ninguém sabe qual é o certo. A geometria está
aqui, com nome e comentário, e os `.png`/`.ico` são saída — versionados porque ícone muda uma vez
por ano e ninguém deveria precisar de Python instalado para compilar o produto.

O desenho: duas telas brancas lado a lado, e o ponteiro já chegou na segunda. É a única coisa que
o produto faz, e é a única coisa que o ícone diz.

A leitura em 16 px foi o critério de projeto. Em 16 px o ponteiro some e sobram dois blocos
brancos separados por uma fenda azul — o que ainda é distintivo. Um ícone que só funciona em 256 px
é um ícone que ninguém vê, porque a barra de tarefas e a lista de programas usam os tamanhos
pequenos.
"""

from __future__ import annotations

import pathlib

from PIL import Image, ImageDraw

AQUI = pathlib.Path(__file__).resolve().parent

# --- Cores -----------------------------------------------------------------------------------
# O mesmo azul de ação de `crates/ir-ui/ui/tema.slint`. O ícone e a interface são o mesmo produto.
AZUL = (37, 99, 235, 255)
AZUL_FUNDO = (29, 78, 216, 255)
BRANCO = (255, 255, 255, 255)

# --- Geometria, num quadrado de 256 -------------------------------------------------------------
LADO = 256
RAIO_DO_FUNDO = 56

# Duas telas de 88x66. A proporção é de monitor, não de quadrado: um retângulo quadrado leria como
# "janela", e o que se quer dizer é "tela".
TELA_LARGURA = 88
TELA_ALTURA = 66
FENDA = 20
TELA_TOPO = 95

# O ponteiro clássico, normalizado numa caixa de 12x19. Escalado na hora de desenhar.
PONTEIRO = [
    (0.0, 0.0), (0.0, 16.0), (4.5, 12.0), (7.0, 18.0),
    (9.5, 17.0), (7.0, 11.0), (12.0, 11.0),
]
PONTEIRO_CAIXA = (12.0, 19.0)
PONTEIRO_ALTURA = 46

# Desenhar em 4x e reduzir dá antisserrilhado sem depender de biblioteca de rasterização.
ESCALA = 4


def gradiente_vertical(lado: int, topo: tuple, base: tuple) -> Image.Image:
    """Um degradê de cima para baixo.

    Duas cores chapadas sobrepostas deixariam as quinas arredondadas do bloco de cima visíveis no
    meio do ícone, o que lê como defeito e não como volume. Um degradê some ao reduzir para 16 px,
    onde ele vira uma cor média -- que é exatamente o que se quer nesse tamanho.
    """
    faixa = Image.new("RGBA", (1, lado))
    pincel = ImageDraw.Draw(faixa)
    for y in range(lado):
        proporcao = y / (lado - 1)
        pincel.point(
            (0, y),
            fill=tuple(
                round(topo[canal] + (base[canal] - topo[canal]) * proporcao)
                for canal in range(4)
            ),
        )
    return faixa.resize((lado, lado))


def desenhar() -> Image.Image:
    """Desenha o ícone em alta resolução."""
    lado = LADO * ESCALA
    imagem = Image.new("RGBA", (lado, lado), (0, 0, 0, 0))
    pincel = ImageDraw.Draw(imagem)

    def e(valor: float) -> float:
        return valor * ESCALA

    fundo = gradiente_vertical(lado, AZUL, AZUL_FUNDO)
    mascara = Image.new("L", (lado, lado), 0)
    ImageDraw.Draw(mascara).rounded_rectangle(
        [(0, 0), (lado - 1, lado - 1)],
        radius=e(RAIO_DO_FUNDO),
        fill=255,
    )
    imagem.paste(fundo, (0, 0), mascara)

    largura_total = TELA_LARGURA * 2 + FENDA
    esquerda = (LADO - largura_total) / 2
    for indice in range(2):
        x = esquerda + indice * (TELA_LARGURA + FENDA)
        pincel.rounded_rectangle(
            [(e(x), e(TELA_TOPO)), (e(x + TELA_LARGURA), e(TELA_TOPO + TELA_ALTURA))],
            radius=e(10),
            fill=BRANCO,
        )

    # O ponteiro fica dentro da tela da direita, encostado na borda por onde ele entrou. Em azul
    # sobre branco: o contraste máximo disponível, que é o que sobrevive à redução.
    escala_do_ponteiro = PONTEIRO_ALTURA / PONTEIRO_CAIXA[1]
    origem_x = esquerda + TELA_LARGURA + FENDA + 12
    origem_y = TELA_TOPO + 9
    pincel.polygon(
        [
            (e(origem_x + px * escala_do_ponteiro), e(origem_y + py * escala_do_ponteiro))
            for px, py in PONTEIRO
        ],
        fill=AZUL_FUNDO,
    )

    return imagem.resize((LADO, LADO), Image.LANCZOS)


# Os tamanhos que os dois sistemas pedem. 22 e 24 são do hicolor do Linux; 16 a 256 são do Windows.
TAMANHOS_PNG = [16, 22, 24, 32, 48, 64, 128, 256]
TAMANHOS_ICO = [16, 24, 32, 48, 64, 128, 256]

# --- A bandeja do Windows ----------------------------------------------------------------------
#
# O ícone ao lado do relógio responde "está acontecendo alguma coisa?" sem abrir nada. Um selo no
# canto de baixo à direita, como o do OneDrive: o arco que gira enquanto algo atravessa, o ✓ quando
# terminou, o ! quando não deu. Sem conexão, o ícone fica cinza.
#
# Uma tira por tamanho de bandeja (100%, 125%, 150% e 200% de escala), os quadros lado a lado, na
# ordem de QUADROS. A ordem é contrato com `crates/ir-ui/src/bandeja/icones.rs`.
QUADROS = ["normal", "inativo", "feito", "problema"] + [f"trabalhando-{i}" for i in range(8)]
TAMANHOS_BANDEJA = [16, 20, 24, 32]

# As cores de significado de `crates/ir-ui/ui/tema.slint`, as do tema claro: a bandeja não sabe o
# tema, e estas têm contraste nos dois.
BOM = (21, 128, 61, 255)
RUIM = (192, 38, 38, 255)

# O selo, num quadrado de 256: o diâmetro, e a folga transparente em volta dele, que o separa do
# ícone mesmo quando as cores se parecem.
SELO = 150
FOLGA = 18


def cinza(imagem: Image.Image) -> Image.Image:
    """O ícone apagado: em tons de cinza e meio transparente."""
    r, g, b, a = imagem.split()
    luz = Image.merge("RGB", (r, g, b)).convert("L")
    a = a.point(lambda v: v * 55 // 100)
    return Image.merge("RGBA", (luz, luz, luz, a))


def com_selo(base: Image.Image, desenhar_selo) -> Image.Image:
    """O ícone com um selo no canto, desenhado em alta resolução por `desenhar_selo`."""
    lado = LADO * ESCALA
    imagem = base.resize((lado, lado), Image.LANCZOS)

    def e(valor: float) -> float:
        return valor * ESCALA

    centro = e(LADO - SELO / 2)
    raio = e(SELO / 2)
    # A folga: um anel apagado do ícone em volta do selo.
    vazio = Image.new("L", (lado, lado), 0)
    ImageDraw.Draw(vazio).ellipse(
        [(centro - raio - e(FOLGA), centro - raio - e(FOLGA)),
         (centro + raio + e(FOLGA), centro + raio + e(FOLGA))],
        fill=255,
    )
    imagem.putalpha(Image.composite(Image.new("L", (lado, lado), 0), imagem.getchannel("A"), vazio))
    desenhar_selo(ImageDraw.Draw(imagem), centro, raio, e)
    return imagem.resize((LADO, LADO), Image.LANCZOS)


def selo_feito(pincel, centro, raio, e):
    pincel.ellipse([(centro - raio, centro - raio), (centro + raio, centro + raio)], fill=BOM)
    pincel.line(
        [(centro - raio * 0.45, centro + raio * 0.02),
         (centro - raio * 0.1, centro + raio * 0.38),
         (centro + raio * 0.48, centro - raio * 0.3)],
        fill=BRANCO, width=round(raio * 0.28), joint="curve",
    )


def selo_problema(pincel, centro, raio, e):
    pincel.ellipse([(centro - raio, centro - raio), (centro + raio, centro + raio)], fill=RUIM)
    largura = raio * 0.26
    pincel.rounded_rectangle(
        [(centro - largura / 2, centro - raio * 0.58), (centro + largura / 2, centro + raio * 0.14)],
        radius=largura / 2, fill=BRANCO,
    )
    pincel.ellipse(
        [(centro - largura / 1.6, centro + raio * 0.3), (centro + largura / 1.6, centro + raio * 0.3 + largura * 1.25)],
        fill=BRANCO,
    )


def selo_trabalhando(quadro: int):
    """O arco de três quartos, girado um oitavo de volta por quadro."""
    def desenhar_selo(pincel, centro, raio, e):
        pincel.ellipse([(centro - raio, centro - raio), (centro + raio, centro + raio)], fill=BRANCO)
        dentro = raio * 0.62
        inicio = -90 + quadro * 45
        pincel.arc(
            [(centro - dentro, centro - dentro), (centro + dentro, centro + dentro)],
            start=inicio, end=inicio + 270, fill=AZUL_FUNDO, width=round(raio * 0.3),
        )
    return desenhar_selo


def bandeja(mestre: Image.Image) -> None:
    quadros = {
        "normal": mestre,
        "inativo": cinza(mestre),
        "feito": com_selo(mestre, selo_feito),
        "problema": com_selo(mestre, selo_problema),
    }
    for i in range(8):
        quadros[f"trabalhando-{i}"] = com_selo(mestre, selo_trabalhando(i))
    for tamanho in TAMANHOS_BANDEJA:
        tira = Image.new("RGBA", (tamanho * len(QUADROS), tamanho), (0, 0, 0, 0))
        for indice, nome in enumerate(QUADROS):
            tira.paste(quadros[nome].resize((tamanho, tamanho), Image.LANCZOS), (indice * tamanho, 0))
        caminho = AQUI / f"bandeja-{tamanho}.png"
        tira.save(caminho, "PNG")
        print(f"  {caminho.name}  ({len(QUADROS)} quadros)")


def main() -> None:
    mestre = desenhar()
    bandeja(mestre)

    for tamanho in TAMANHOS_PNG:
        caminho = AQUI / f"icone-{tamanho}.png"
        mestre.resize((tamanho, tamanho), Image.LANCZOS).save(caminho, "PNG")
        print(f"  {caminho.name}")

    ico = AQUI / "icone.ico"
    mestre.save(ico, "ICO", sizes=[(t, t) for t in TAMANHOS_ICO])
    print(f"  {ico.name}  ({len(TAMANHOS_ICO)} tamanhos)")


if __name__ == "__main__":
    main()
