# Cada computador com os seus cards

**Data:** 2026-10-27 · **Branch:** `feat/painel-android`

**Itens:** [PROGRESSO](../../PROGRESSO.md), painel no Android.

**O que foi pedido:** "quando adiciono um card não dá para saber para qual PC está sendo adicionado;
deveria ter o botão de adicionar conforme o computador."

## O que havia

Um painel tinha **um** arranjo, valendo para todos os computadores. "Adicionar card", na barra de
cima, oferecia a mistura dos cards de todos; tirar, mover ou destacar num computador mudava o outro
junto. Os processos que o painel já pede ao computador (chrome, code, docker, wsl) apareciam
misturados aos cards de sempre.

## O que mudou

- **Cada computador tem os seus cards** (`PainelPersonalizado.porMaquina`, pelo nome, sem maiúscula).
  O `arranjo` do painel guarda as faixas de baixo (enlace e cópia), que são dos dois, e é o ponto de
  partida de um computador ainda não arrumado: o primeiro toque nele copia e muda a cópia. Um painel
  salvo antes continua igual nos dois computadores até a pessoa mexer.
- **"+ Adicionar card" no cabeçalho de cada computador**, ao lado de "Inverter lado". O diálogo diz o
  nome ("Adicionar card em fedora") e mostra só o que **aquele** computador mede e não está na tela,
  em grupos: os cards de sempre; os processos, com "Acompanhar outro processo…"; os sensores.
- **"Acompanhar processo"** — no diálogo e na lista de processos — vale para o computador da lista.
- **A barra de cima ficou com o que é do painel**: abas, inverter lados, "Restaurar padrão" (dos dois
  computadores) e, só quando uma faixa foi tirada, "+ Entre os computadores" / "+ Cópia de arquivos".
- **Uma só porta de edição**: `Aparelho.editarPainel`, com `editarArranjo(maquina, …)` por cima. O
  "Desfazer" guarda o painel inteiro antes de cada passo e devolve só os cards (um nome trocado no meio
  fica). Chave nova vinda do computador entra por fora do "Desfazer".
- Os processos pedidos ao computador são os de todos os arranjos de todos os painéis.

## O que foi provado

- Testes novos de `PainelPersonalizado`: cada computador isolado, editar sem mudança devolve o mesmo
  painel, chave nova entra no computador que a mandou, restaurar volta os dois, processos de todos, e
  os cards por computador sobrevivem ao JSON. `testDebugUnitTest` verde.
- No tablet, com fedora e SAMSUNG-MAXUEL conectados: cada um com o seu "+ Adicionar card"; docker
  adicionado no fedora apareceu só nele; "Desfazer" o tirou e sumiu.

## Observação

O aplicativo do tablet foi instalado do zero às 15:09 de hoje (`firstInstallTime`) — fora desta
sessão, que só atualiza com `install -r` —, e uma instalação nova apaga os dados: o painel estava no
padrão antes desta mudança.
