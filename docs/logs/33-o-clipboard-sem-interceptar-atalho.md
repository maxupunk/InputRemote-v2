# O clipboard sem interceptar atalho, e o laço que a guarda desfaz

**Data:** 2026-09-17

**Itens:** Etapa 8 — `ir-clip`, em andamento: texto e arquivos, com a guarda de eco.

**O que foi feito:** o crate do clipboard. A parte pura está pronta e testada; o backend do Windows
está escrito e compila; o do Linux declara o que falta em vez de fingir.

## A decisão que organiza o crate: não interceptar Ctrl+C

O produto já tem ganchos de teclado no Windows. Seria possível reconhecer Ctrl+C e Ctrl+V ali. Está
errado, por três razões que não se resolvem com mais código:

1. **Não é o atalho que copia, é o aplicativo.** Ctrl+C no Explorer põe `CF_HDROP`; no editor, texto;
   numa tela de desenho, imagem. Quem sabe o que copiar é ele.
2. **O atalho não é universal.** Copiar pelo menu, Ctrl+Insert, arrastar, ou o Ctrl+C de um terminal
   que o trata como interrupção — nada disso passaria por um reconhecedor de combinação.
3. **O sistema já avisa.** `AddClipboardFormatListener` existe exatamente para dizer "o clipboard
   mudou".

Então o desenho é espelhar: o clipboard local mudou, oferecemos ao par; o par ofereceu, publicamos
localmente. **Nenhum código nosso corre no caminho da colagem** — o Ctrl+V do usuário é tratado pelo
aplicativo dele, lendo o clipboard que já está lá. É por isso que colar é instantâneo, e não "quase
instantâneo".

## O laço que espelhar cria

Espelhar nos dois sentidos é um laço infinito por construção:

```text
A: Ctrl+C                    →  A oferece a B
B: publica no clipboard
B: o ouvinte de B dispara    →  B oferece a A
A: publica no clipboard
A: o ouvinte de A dispara    →  ...
```

Cada volta atravessa a rede e reescreve o clipboard das duas máquinas. A guarda (`eco.rs`) lembra o
resumo BLAKE3 do que publicamos e engole qualquer mudança com aquele resumo.

**Qualquer**, e não só a primeira. No Windows, publicar um conteúdo pode disparar
`WM_CLIPBOARDUPDATE` mais de uma vez para a mesma cópia, e engolir só o primeiro deixaria o laço
passar pelo segundo. Há teste para as duas coisas.

## O detalhe que quase passou: a quebra de linha

O protocolo é LF; o Windows quer CRLF. Se o `Conteudo` guardasse o que o sistema deu, este seria o
caminho:

```text
publicamos "a\nb"  →  o sistema guarda "a\r\nb"  →  lemos "a\r\nb"  →  resumo diferente
                   →  a guarda não reconhece a própria cópia  →  o laço acontece
```

A correção é o tipo ser **canônico por construção**: `Conteudo::texto` normaliza na entrada, e o
backend converte só na saída. Está fixado por um teste que faz a ida e volta inteira —
canônico → nativo → canônico — e por outro que publica LF e oferece de volta o CRLF que o sistema
devolveria.

## Duas escolhas de tipo que valem registro

**`Vigia` não é `Send`, e é de propósito.** `GetMessageW` lê a fila da thread que criou a janela. Um
vigia criado numa thread e bombeado em outra **não dá erro** — fica calado para sempre, que é a forma
mais difícil possível de descobrir o problema. Sem `Send`, o compilador obriga a criação a acontecer
na thread que vai esperar. Um `unsafe impl Send` compilaria e esconderia isso.

**Arquivos antes de texto, ao ler.** O Explorer põe os dois formatos ao copiar arquivo: `CF_HDROP`
com a lista e `CF_UNICODETEXT` com os nomes. Olhar texto primeiro transformaria "copiei um arquivo"
em "copiei o nome de um arquivo".

## O que o Linux diz, em vez de fingir

`linux.rs` devolve `Indisponivel` com a frase do que falta. No Wayland não há "o clipboard"
alcançável de fora: ou `wlr-data-control` — que **o GNOME não expõe**, e a bancada é GNOME — ou o
portal `org.freedesktop.portal.Clipboard`, que exige uma sessão de portal viva e o D-Bus.

E a distinção está no tipo: `ClipError::e_ausencia()` separa "suspenso, e aqui está o motivo" de
"deu problema". No desktop `Winlogon` o clipboard também não existe, e ali o produto está **correto**
ao não sincronizar — mostrar isso como erro faria parecer quebrado.

## Arquivos

`crates/ir-clip/` novo: `conteudo`, `eco`, `error`, `linux`, `windows/{mod,area,vigia}`. Mais
`Cargo.toml` do workspace e `PROGRESSO.md`.

**Verificação:** 36 testes no `ir-clip`, todos passando; workspace verde; clippy silencioso; `xtask`
nos três critérios, 197 arquivos. O `unsafe` fica em dois módulos, cada um com a justificativa no
topo.

**O que ainda não foi provado:** nada do backend do Windows foi exercitado contra um clipboard de
verdade — os 36 testes cobrem a parte pura e a montagem dos bytes do `DROPFILES`, não as chamadas ao
sistema. E o Ctrl+C ainda não dispara nada: falta ligar o crate ao agente
(`ComandoDoAgente::PublicarClipboard`, `FatoDoAgente::ClipboardMudou`, a thread do vigia). No Windows
esse teste volta a depender do MSI instalado, porque o canal do agente é restrito a SYSTEM
([log 29](29-o-agente-que-nunca-dizia-por-que.md)).
