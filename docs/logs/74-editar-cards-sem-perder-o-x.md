# Editar os cards sem perder o ✕

**Data:** 2026-10-27 · **Branch:** `feat/painel-android`

**Itens:** [PROGRESSO](../../PROGRESSO.md), painel no Android.

**O que foi pedido:** "o botão ✕ para remover está ficando de fora dos pequenos cards; preciso que
seja mais amigável para edição — a melhor experiência de usuário para o caso."

## O que havia

Cada card em edição levava embaixo uma linha de quatro botões de 44 dp (◀ ▶ ★ ✕): 176 dp. O card
compacto tem 140 dp de largura mínima, e no tablet deitado, com dois computadores lado a lado, fica
com uns 190 dp menos as margens — o ✕, o último, era cortado. Nas faixas (enlace e cópia), que
ocupam a altura toda, a linha nem aparecia. E um toque errado no ✕ só se consertava achando o card
de novo em "Adicionar card" — o de processo, refazendo o "Acompanhar".

## O que mudou

- **O ✕ virou um selo vermelho no canto de cima do card**, dentro dele — para nenhuma rolagem o
  cortar —, com 24 dp de desenho e 40 dp de toque. Está em todo card e em toda faixa, por menor que
  seja.
- **Embaixo ficaram ◀ ★ ▶, repartindo a largura que houver**, na ordem em que se lê: antes, destaque,
  depois. Cabem em qualquer card.
- **"Desfazer" ao lado de "Concluir"**: cada mudança da edição (tirar, mover, destacar, adicionar,
  restaurar o padrão) guarda o arranjo de antes, por painel; o botão volta um passo por vez e some
  quando não há o que voltar. Ao concluir, o histórico é esquecido. Tirar ficou seguro sem pedir
  confirmação a cada toque. O desfazer salva pelo editar de fora, para não virar ele mesmo um passo.
- O texto da barra diz onde está cada coisa; o "Concluir" repetido no fim da fila rolável saiu — o de
  cima está sempre à vista.

## O que foi provado

- No tablet (1280×800, deitado, dois computadores): todos os selos à vista, inclusive nos compactos e
  nas faixas; tirar um card de sensor e desfazer o devolve no mesmo lugar, e o botão some.
- `testDebugUnitTest` e o APK de release, assinado com a chave de teste.
