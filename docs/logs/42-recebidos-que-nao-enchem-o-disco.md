# Recebidos que não enchem o disco, e o arquivo que chega com o nome que saiu

**Data:** 2026-09-21

**Itens:** Etapa 8 — copiar e colar de ponta a ponta; Etapa 9 — a janela.

**O que foi feito:** dois relatos com a mesma raiz. A pasta `/var/lib/inputremote/recebidos` tinha
8 GB, e tudo o que chegava vinha com o nome alterado — `FIMI0022.LRV (2)`, `(3)` — mesmo com a
pasta de destino vazia.

## A causa, que é uma só

O que chega precisa existir em algum lugar para poder ser colado. Ficava em `recebidos`, e **nada
nunca saía de lá**. Como o nome já estava ocupado pela entrega anterior, a publicação escolhia o
"primeiro nome livre" — `(2)`, `(3)` — e era esse nome que o usuário colava do outro lado. Uma
regra alimentava a outra: pasta que só cresce, nome que sempre muda.

## O que mudou

**O recebido mantém o nome.** A entrega nova é *a versão nova daquilo*: ela toma o lugar da
anterior de mesmo nome. A anterior é afastada antes (`nome.anterior-do-inputremote`) e só apagada
depois de o `rename` da nova dar certo — se algo falhar no meio, o usuário fica com a antiga, que é
melhor que ficar sem nenhuma.

**A pasta começa vazia.** Ao subir, o serviço esvazia `recebidos`. O clipboard não sobrevive ao
desligamento: nada do que ficou de uma sessão anterior ainda vai ser colado, então guardar é só
ocupar disco. É a regra mais simples que existe, e é a que o usuário espera de uma pasta de
trabalho temporária.

**Durante a sessão, a pasta se cuida sozinha** (`ir-transferencia/src/faxina.rs`), com a política de
uma pasta de downloads e três limites, nesta ordem:

1. as **três mais novas nunca saem** — a que acabou de chegar é a que a pessoa vai colar;
2. o que passou de **duas semanas** sai, tenha o tamanho que tiver;
3. se ainda passar de **2 GB**, sai da mais velha para a mais nova até caber.

A decisão é uma função pura (`escolher`), que não toca o disco: é ela que tem teste, e é por isso
que a política inteira é testável sem criar 8 GB de arquivos. A faxina roda depois de **cada
entrega** — o momento em que a pasta acabou de crescer.

**E o botão, porque nem tudo é automático.** Preferências mostra quanto está ocupado e oferece
"Limpar agora", que esvazia tudo — inclusive o que ainda não foi colado, que é justamente o que o
automático **não** faz sozinho. O serviço responde na hora e apaga fora do compasso do ator; o
tamanho de volta chega pelo aviso de estado.

## Uma fronteira que o limite apontou

O serviço passou das 2 500 linhas de produção de `docs/09` §1. O limite não é um número: é o aviso
de que falta uma fronteira. Ela estava à vista — lançar o agente e o ajudante **dentro da sessão do
usuário** no Windows (duplicar token, mover para a sessão de console, montar o ambiente, escolher o
desktop) não é assunto do serviço, que só decide *quando* quer um. Virou o crate `ir-sessao`, com o
contador de ajudantes junto, que só existe por causa dele.

## A prova

Testes novos: a política de faxina, caso a caso (pasta pequena não perde nada; o que passou da
idade sai; as mais novas ficam mesmo estourando o teto; estourando o espaço sai da mais velha até
caber; a ordem de apagar é da mais velha para a mais nova); e o esvaziar contando arquivo e árvore.

Testes que **mudaram de lado**, porque a regra mudou: `staging` e a travessia de ponta a ponta
fixavam "duas entregas com o mesmo nome não se sobrescrevem" e agora fixam o contrário, com o
motivo escrito no teste — inclusive que nada sobra ao lado (nem `(2)`, nem a cópia afastada).

**Bancada:** _a preencher._

**Verificação:** 893 testes no Windows; `cargo xtask check`.
