# O painel deitado, e os cards que a pessoa arruma

**Data:** 2026-10-24 · **Branch:** `feat/painel-android`

**Itens:** [12, §5.2](../12-painel-android.md); [PROGRESSO](../../PROGRESSO.md), Etapa 12.

**O que foi pedido:** "melhorar o layout e pensar em uma interface deitada, onde primeiro deve ter a
CPU, memória como mais importante; fazer com que o usuário possa alterar, adicionar e apagar os
cards."

## O que havia

No tablet de verdade (M8_4G, 240 dpi: 853 × 533 dp deitado), o layout do [log 68](68-o-painel-no-android.md)
punha todos os cards do mesmo tamanho, a 160 dp. A memória ficava embaixo do processador, e o resto
saía da tela.

## O que mudou

- **`Arranjo`** (`android/.../dados/Arranjo.kt`), puro e imutável: os cards visíveis, em ordem, cada
  um normal ou em destaque, e as chaves já vistas. Só conhece a `chave` que o núcleo mantém estável,
  então vale para qualquer métrica: uma nova entra no fim, visível; uma tirada continua conhecida para
  voltar. Padrão: processador e memória em destaque, depois a temperatura do processador, a rede, o
  disco, a bateria, as outras temperaturas e a faixa do enlace.
- **Guardado no aparelho** (`Preferencias`, em JSON por `ArranjoJson`), e editado por um caminho só,
  `editarArranjo { it.mover(chave, -1) }`: a tela diz o que muda, e nada se repete por ação.
- **`Acoes` ficou só com a ligação**, e o que é do aparelho (arranjo, escurecer, girar) foi para
  `Aparelho`. O protetor de tela passa um `Aparelho` só para ler.
- **Dois tamanhos de card** no mesmo componente: destaque (36 sp, barra grossa, gráfico) e compacto
  (uma linha densa, valor e detalhe lado a lado). O valor nunca é cortado; o detalhe encolhe.
- **Deitado por padrão**, nos dois sentidos; "Girar com o aparelho" no menu.

## O que foi provado

- **19 testes de JVM**, 8 deles novos (o arranjo e a gravação dele).
- **No tablet de verdade**, contra a bancada: o processador e a memória lado a lado no topo de cada
  computador; o resto cabendo sem rolar; o menu com "Editar painel"; os controles ◀ ▶ ★ ✕ em cada
  card; "Adicionar card" oferecendo exatamente o que foi tirado.
- **A edição pelas mãos de quem estava com o tablet**: disco, temperatura e a faixa do enlace foram
  tirados por lá, e o arquivo de preferências do aplicativo os mostra fora da tela e ainda conhecidos —
  e o arranjo sobreviveu a duas reinstalações.

## O que deu errado no caminho

- Com 200 dp de largura mínima para o destaque, a coluna de ~380 dp cabia um só: o processador e a
  memória empilhavam. Baixou para 170 dp.
- O valor da rede saía cortado ("↓ 1,7 MB/"), porque o detalhe dividia o espaço com ele.
- "Entre os computadores" saía cortado na faixa.
- Um teste meu esperava o fim errado ao mover o primeiro card para o último lugar.
