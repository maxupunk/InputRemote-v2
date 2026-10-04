# O cartão da pasta, mais baixo, e esvaziar a lixeira

**Data:** 2026-10-03

**Itens:** [ADR-0015](../adr/0015-pastas-compartilhadas.md) — pastas compartilhadas;
[log 59](59-a-pasta-sob-demanda.md) — a lixeira da pasta; [PROGRESSO](../../PROGRESSO.md), Etapa 9.

**O que foi pedido:** em "Pastas compartilhadas", o cartão de cada pasta mais baixo, ocupando menos
espaço, e um botão de "esvaziar a lixeira".

## O cartão

Antes, cada pasta empilhava cinco faixas: o nome, a situação, o detalhe, e uma fileira de botões de
altura cheia. Com um conflito, vinha ainda uma faixa com botão próprio, e o terceiro botão do
conflito ("Só a de NOTEBOOK") saía cortado à direita.

Agora:

| Onde | O que |
|---|---|
| Em cima, à esquerda | o ponto, o nome, a situação (letra de nota) e o detalhe em uma linha só — o caminho longo termina em "…" |
| Em cima, à direita | **Abrir**: o que mais se faz com uma pasta fica ao lado do nome |
| Embaixo | **Lixeira** e **Esvaziar lixeira** à esquerda; **Parar de compartilhar** (segurar) à direita, longe dos dois |

- O cartão com a pasta em dia caiu de 147 para 107 px na janela de 476 px: cerca de um quarto.
- O conflito perdeu o botão "Ver as duas" da faixa: ele abria a pasta, o mesmo que **Abrir**, logo
  acima. A faixa fica, só com a frase.
- Os três botões do conflito agora cabem, e a nota dos 30 dias ficou em uma linha.

**Peça reaproveitada, e não copiada.** `Cartao`, `Botao` e `BotaoDeSegurar` ganharam `compacto`:
menos respiro no cartão, 28 px em vez de 34 e a letra de nota nos botões. `BotaoConfirmado` herda de
`Botao` e ganha o mesmo de graça. Nenhum componente novo; as outras telas não mudam.

## Esvaziar a lixeira

- **O botão.** É o `BotaoConfirmado`, o mesmo de "Limpar agora" nas Preferências: o primeiro clique
  troca o texto para "Apagar de vez?", e só o segundo, em até 4 segundos, apaga. Esvaziar tira o
  que a lixeira guardava justamente para desfazer, então não pode ser um clique só.
- **Apagado quando vazia.** O resumo da pasta ganhou `lixeira_com_algo`. Com a lixeira vazia, o
  botão aparece apagado, em vez de "esvaziar" o nada. O ajudante refaz o resumo a cada volta do
  laço, então o botão acompanha a lixeira sozinho.
- **Só neste computador.** Cada lado tem a sua lixeira — a origem em `<pasta>/.inputremote/lixeira`,
  a réplica no diretório de estado —, e nada disso sincroniza. Esvaziar aqui não toca a de lá.

**Como, peça por peça:**

- **`ir-ipc`.**
  - `ComandoDePasta::EsvaziarLixeira(IdDePasta)`, no fim do enum, com o número no fio fixado no
    teste.
  - `ResumoDePasta.lixeira_com_algo`, no fim, com `serde(default)`.
- **`ir-acervo/src/disco.rs`.**
  - `esvaziar` e `tem_algo`.
  - A faxina dos 30 dias e o esvaziar passam pela mesma função, `tirar_da_lixeira`, que só difere no
    que escolhe tirar: o velho, ou tudo.
  - Um arquivo aberto no Windows não segura o resto: tudo o que dá é tirado, e o primeiro erro volta.
- **`ir-sincronia`.**
  - O comando, com a frase de sempre quando falha: o que houve e o que fazer ("Feche o que estiver
    aberto de lá e tente de novo").
  - Abrir e esvaziar a lixeira passaram para funções próprias: o despacho dos comandos voltou para
    baixo das 60 linhas.
- **`ir-ui`.**
  - `PastaUi.lixeira` e a ação `esvaziar-lixeira`, ligada pela mesma `por_posicao` das outras.
  - Na demonstração, "Projetos" tem algo na lixeira e "Fotos" não, para as duas formas do botão
    aparecerem.

## Verificação

- **Teste novo** em `disco.rs`: a lixeira que não existe está vazia, e esvaziá-la não é erro; com um
  dia guardado e um arquivo solto, esvaziar tira os dois e a lixeira em si fica.
- **Windows:**
  - `cargo xtask check`, `cargo clippy --workspace --all-targets -D warnings` e `cargo fmt` limpos;
  - `cargo test --workspace`: nenhuma falha.
- **Linux** (contêiner Fedora 44 com `/dev/fuse`): clippy limpo; 1 348 testes passando, nenhuma falha.
- **A tela, na demonstração (`--simulado --pareado`), fotografada:**
  - os cartões compactos, com os três botões do conflito inteiros;
  - "Esvaziar lixeira" apagado em "Fotos";
  - em "Projetos", o primeiro clique mostra "Apagar de vez?".
- **Não verificado ainda:** esvaziar com o serviço instalado, nas duas máquinas — a réplica sob
  demanda do Linux e a raiz do Explorer no Windows.
