# O aparelho pede para parear

**Data:** 2026-10-26 · **Branch:** `feat/painel-android`

**Itens:** [12, §4 e §5.4](../12-painel-android.md); [04, §3.2](../04-seguranca.md);
[PROGRESSO](../../PROGRESSO.md), Etapa 12.

**O que foi pedido:** "uma forma mais simples de parear, sem ter que ir em 'Painel no celular' para
conectar; a melhor experiência de usuário possível". Entre as três saídas apresentadas, a escolhida
foi **o tablet pede e o PC confirma**.

## O que havia

Para parear, a pessoa ia primeiro ao computador: "Painel no celular" → "Parear aparelho" abria a
porta 52527 por três minutos. Só depois o "Parear" do aparelho achava quem atender. Na bancada isso
virou o defeito relatado: a porta estava fechada no fedora ("Connection refused (os error 111)", em
inglês e com número de erro na tela do tablet), e a única porta aberta era a da bancada, no Samsung
— que mostrava um "notebook (simulado)".

## O que mudou

- **O pedido parte do aparelho.** Com `Comando::AceitarPedidos(true)` o monitor deixa a porta aberta
  enquanto o serviço roda, e um aparelho novo chega ao código sem ninguém ter aberto nada no
  computador. A janela do PC vem para a frente sozinha na comparação — também da bandeja — e um
  recado do sistema diz "Um aparelho quer ver o uso deste computador", com o código.
- **O código continua nas duas telas.** É ele que impede alguém no meio da rede de se passar pelo
  computador; pular a confirmação no PC deixaria qualquer aparelho do Wi-Fi ver o uso e os nomes dos
  processos. Recusado.
- **O que a porta aberta exige em troca:**
  - um pareamento de cada vez (já era assim);
  - o código espera **60 s**, e não mais 3 min (`PRAZO_DO_CODIGO`): ninguém aqui pediu, e ele não
    pode ficar na tela;
  - **três pareamentos sem fim** — recusados, sem resposta, abandonados — deixam aquele endereço
    de fora por **10 min** (`recusas.rs`): um aparelho insistente não vira uma janela sem fim;
  - um interruptor, **"Aparelhos novos: Podem pedir / Só pelo botão"** na tela do painel no PC,
    gravado em `painel_aceita_pedidos` (ligado por padrão). Desligado, volta o fluxo antigo, com o
    botão "Parear aparelho".
- **O interruptor anda pelo canal dos painéis** (`ComandoDePainel::AceitarPedidos` e
  `ComoEstaoOsPedidos`, `AvisoDoPainel::AceitaPedidos`, no fim dos enums), resolvido inteiro no
  `ir-vigilancia`: o `ir-daemon` está a dez linhas do teto e não foi tocado.
- **No aparelho**, a tela de procura diz o que fazer ("Toque em Parear ao lado do seu computador. O
  código aparece na tela dele"), e o erro de conexão virou frase: "O computador 10.0.0.135 não
  atendeu o painel. Confira se o InputRemote dele está atualizado e aceitando aparelhos novos".
- **A bancada** passou a aceitar pedidos, como o serviço, em vez de reabrir a janela a cada 2 min.

## O que foi provado

- Teste em TCP de verdade: com pedidos aceitos e sem janela, o aparelho chega ao código, e o
  computador mostra o mesmo; os do castigo por endereço; o interruptor gravado e contado à janela
  mesmo sem o monitor de pé; o texto do recado com o código em três e três.
- `clippy` sem aviso e os testes dos crates tocados no Windows e no Fedora 44; `cargo xtask check`.
- A tela do PC em `--simulado`: as duas posições do interruptor, o texto dos passos mudando com ele,
  e o botão "Parear aparelho" só quando os pedidos estão desligados.
- O APK de release, pela chave do `empacotar`, instalado por cima do que estava no tablet, sem perder
  o pareamento nem os cards.

## O que deu errado no caminho

- O nome do campo da configuração tinha "paineis", e um teste confere que uma lista vazia de painéis
  não vai para o arquivo procurando essa palavra: virou `painel_aceita_pedidos`.
- O `clippy` pegou uma indexação no leitor de processos do Windows do [log 70](70-processos-e-copia-no-painel.md),
  reescrito depois da última passada dele.
- O APK de depuração não instala por cima do de release (chaves diferentes); o Android recusa sem
  apagar nada, e o de release, pelo `empacotar`, entrou no lugar.

## O que falta

- Com o **serviço instalado** desta versão nos dois computadores (o MSI e o RPM de `dist/`): parear
  o tablet com um deles sem abrir a janela, ver a janela vir para a frente com o recado, e o outro
  computador aparecer como segunda máquina pela sessão.
