# O ícone, e a janela preta que abria atrás

**Data:** 2026-09-10

**Itens:** Etapa 9 — identidade visual do programa. Etapa 1.5 — o ícone nos dois instaladores.

## A janela de console

Um binário do Windows nasce no subsistema `console`, e o sistema abre um terminal para ele. Num
programa de janela isso é uma janela preta que aparece junto, fica atrás e não serve para nada.

A correção é um atributo:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
```

**Só no release.** Em `debug` o console continua vindo, porque durante o desenvolvimento ele é
onde pânico e `eprintln!` aparecem — e trocar isso por uma janela mais limpa seria péssimo
negócio. No release quem guarda esse tipo de coisa é o registro em arquivo, que é a Etapa 1.4.

Verificado lendo o cabeçalho PE dos dois binários: `release` diz `GUI`, `debug` diz `CONSOLE`.

## O ícone

Duas telas brancas lado a lado, e o ponteiro já chegou na segunda. É a única coisa que o produto
faz, e é a única coisa que o ícone diz.

**O critério de projeto foi a leitura em 16 px**, e não a beleza em 256. A barra de tarefas e a
lista de programas usam os tamanhos pequenos; um ícone que só funciona grande é um ícone que
ninguém vê. Em 16 px o ponteiro some e sobram dois blocos brancos separados por uma fenda azul —
o que ainda é distintivo, que é o que se precisava.

Detalhes que vieram desse critério:

- as telas são retângulos de proporção de monitor, não quadrados: quadrado leria como "janela";
- o ponteiro é azul sobre o branco da tela, o contraste máximo disponível, porque é ele que
  sobrevive à redução;
- o azul é o mesmo `acao` de [`tema.slint`](../../crates/ir-ui/ui/tema.slint) — o ícone e a
  interface são o mesmo produto.

O fundo começou com dois azuis chapados sobrepostos, e as quinas arredondadas do bloco de cima
apareciam no meio do ícone. Aquilo lê como defeito, não como volume. Virou degradê, que ao reduzir
para 16 px se transforma numa cor média — exatamente o que se quer nesse tamanho.

## Uma fonte só

[`recursos/gerar-icones.py`](../../recursos/gerar-icones.py) **é** o ícone. Não existe um `.svg` ao
lado dele, e isso é decisão, não preguiça: dois arquivos com o mesmo desenho sempre acabam
diferentes, e aí ninguém sabe qual é o certo. A geometria está no script, com nome e comentário.

Os `.png` e o `.ico` são saída, e mesmo assim ficam versionados — ícone muda uma vez por ano, e
ninguém deveria precisar de Python instalado para compilar o produto.

O script usa Pillow e desenha em 4× antes de reduzir, o que dá antisserrilhado sem depender de
rasterizador de SVG instalado na máquina.

## Três lugares, três caminhos

O mesmo ícone precisa chegar por caminhos diferentes, e cada um é obrigatório:

| Onde aparece | Como chega |
|---|---|
| Explorer, propriedades do arquivo | recurso embutido no `.exe`, por `winresource` no `build.rs` |
| Barra de tarefas, título da janela | `icon:` no `Window` do [`app.slint`](../../crates/ir-ui/ui/app.slint) |
| Aplicativos Instalados | `ARPPRODUCTICON` no MSI, a partir do `.ico` |
| Lançador do Linux | `Icon=inputremote` no `.desktop`, resolvido pelo tema `hicolor` |

Faltar um deixa o ícone genérico exatamente naquele lugar, e é o tipo de coisa que ninguém nota
até estar na frente de outra pessoa.

No Linux o RPM instala **um arquivo por tamanho** em `/usr/share/icons/hicolor/<n>x<n>/apps/`. Um
PNG grande sozinho obrigaria cada lançador a reduzir por conta própria, e cada um reduz de um
jeito. O pacote passou a exigir `hicolor-icon-theme`, que é quem resolve `Icon=inputremote` para um
arquivo.

O `build.rs` embute o recurso apenas quando o **alvo** é Windows, e não quando a máquina que
compila é: `cfg(windows)` num build script fala do compilador, não do produto, e numa compilação
cruzada de Windows para Linux isso produziria um binário inválido.

De brinde, o recurso do Windows carrega `ProductName` e `FileDescription`, que é o que aparece nas
propriedades do arquivo e no Gerenciador de Tarefas — melhor que "inputremote-ui.exe" sozinho.

## Arquivos

`recursos/gerar-icones.py`, `recursos/icone.ico`, `recursos/icone-{16,22,24,32,48,64,128,256}.png`,
`crates/ir-ui/{build.rs,Cargo.toml,src/main.rs,ui/app.slint}`,
`empacotar/windows/Produto.wxs`, `empacotar/empacotar.ps1`,
`empacotar/linux/{inputremote.spec,inputremote.desktop}`.

## O `BuildRequires` fez o que existe para fazer

Ao acrescentar `desktop-file-validate` ao `%check` do RPM, declarei `BuildRequires:
desktop-file-utils` — e a imagem de compilação não tinha o pacote. O `rpmbuild` recusou a
construção **em dois segundos**, antes de começar os sete minutos de `cargo build`.

Sem a declaração, o `%check` teria falhado no fim, depois da compilação inteira. Vale registrar
porque a tentação, num `%check`, é escrever `|| :` para "não atrapalhar a build" — e aí a
verificação deixa de existir sem ninguém perceber. O `|| :` que estava lá saiu.

## Verificação

- cabeçalho PE: `release` = `GUI`, `debug` = `CONSOLE`;
- o executável de release foi aberto: a janela sobe com o título "InputRemote", nenhum `conhost`
  novo aparece, e o stderr fica vazio;
- `ExtractAssociatedIcon` devolve o ícone embutido, e `FileVersionInfo` mostra `ProductName` e
  `FileDescription`;
- `winresource` é MIT, licença já permitida em `deny.toml`;
- `cargo fmt`, `cargo clippy --all-targets` e `cargo xtask check` limpos;
- o MSI foi refeito e inspecionado pelo próprio banco de dados: `icone.ico` na tabela `Icon`,
  `ARPPRODUCTICON` apontando para ele, e o executável de 9,29 MB dentro do CAB;
- o RPM foi refeito e **instalado num `fedora:44` limpo**: os oito tamanhos aparecem em
  `/usr/share/icons/hicolor/<n>x<n>/apps/inputremote.png`, o `.desktop` traz `Icon=inputremote`,
  o `hicolor-icon-theme` veio junto como dependência, e `dnf remove` sai sem resíduo.
