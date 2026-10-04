# A tela inicial mais leve: a borda vai para Preferências, e o tráfego aparece

**Data:** 2026-10-03

**Itens:** [ADR-0014](../adr/0014-controle-simetrico.md) — a borda que vale é a escolhida por último;
[log 60](60-a-copia-e-a-pasta.md) — a cópia e a pasta; [PROGRESSO](../../PROGRESSO.md), Etapa 9.

**O que foi pedido:**

1. Tirar "Onde fica o outro computador" da tela inicial e pô-lo em Preferências.
2. Mostrar na tela inicial o tráfego usado, para saber se algo está acontecendo, com ótima
   usabilidade e sem informação demais.

Seguindo os princípios SOLID e DRY.

## O que mudou

### A borda em Preferências

O cartão inteiro saiu da tela inicial e foi para Preferências, logo depois de "Quem pode
controlar": o mapa das duas telas, "Borda livre / Borda travada" e a frase dos atalhos. É
configuração que quase nunca muda, e na tela inicial ocupava o maior bloco.

O componente é o mesmo (`EscolhaDeBorda`, em `bordas.slint`); só mudou de lugar. Continua aparecendo
só com par.

### O tráfego, numa linha

No cartão do estado, logo abaixo da conexão, uma linha nova responde "está fazendo algo agora?":

| Situação | A linha |
|---|---|
| Uma cópia ou a pasta recebendo e mandando | `Tráfego   ↓ 2,2 MB/s  ↑ 15,1 KB/s`, na cor de ação |
| Só um sentido andando | só ele: `↓ 2,2 MB/s` |
| Nada atravessando (abaixo de 1 KB/s, a conversa de manutenção do canal) | `Tráfego   parado`, em texto fraco |

Não há total nem gráfico: o total não diz se algo está acontecendo agora, e o gráfico da tela já
é o do atraso. O total da sessão que existia ("nesta sessão: 1,2 GB", no cartão da cópia) saiu. Ele
só contava as cópias que terminavam, e não via a pasta compartilhada; a linha nova cobre as duas.

## Como, camada por camada

Cada peça faz uma coisa só:

- **`ir-transporte`, `dados.rs`: contar.** `Contador` tem duas contagens atômicas, enviados e
  recebidos, do quadro cifrado (o que a rede carrega). A `Porta` o recebe na abertura e o entrega a
  cada enlace que abre; o `Remetente` e o `Destinatario` somam a cada quadro. Uma contagem só para
  cópias e pastas, porque as duas viajam no mesmo canal.
- **`ir-transferencia`: guardar e entregar.** Cria o contador e o passa à porta, que pode ser
  reaberta sem perder a contagem. Também o expõe pela alça que o serviço já tem
  (`Pedidos::trafego()`).
- **`ir-ipc`, `status.rs`: levar.** `Estado.trafego: Trafego { enviados, recebidos }`, no fim da
  estrutura (o `postcard` é posicional) e com `serde(default)`.
- **`ir-painel` e `ir-daemon`: pôr no retrato.** O retrato do ator, como os demais campos.
- **`ir-ui`, `trafego.rs` (novo): a taxa e a frase.** A janela já lê o estado a cada segundo;
  `MedidorDeTrafego` faz da diferença entre duas leituras a taxa de cada sentido.

  Ele não reaproveita o `Velocimetro` da cópia de propósito: aquele suaviza para a velocidade de uma
  cópia ser legível, e levaria uns vinte segundos para mostrar "parado". Aqui, nada no último
  intervalo é "parado" na hora. A escrita da taxa é a mesma do cartão da cópia
  (`historico::por_segundo`), para os dois números saírem iguais.
- **Telas.** `dados.slint` ganha `trafego-ativo`; `inicio.slint` mostra a linha;
  `preferencias.slint` recebe a borda.

Para caber no teto de linhas, a parte da cópia de arquivos da janela (o cartão, a velocidade, a lista
das últimas) saiu de `janela.rs` para `janela/copias.rs`, ao lado de `janela/pastas.rs`.

A demonstração ganhou `--simulado --pareado`: a tela inicial de uma vez, já conectada, com o canal
de dados andando em rajadas de uns dez segundos. É por ela que a tela foi revista.

## Verificação

- Os testes novos:
  - o contador soma o que sai e o que chega, num socket de verdade (`ir-transporte`);
  - o medidor mostra só o sentido que anda, diz "parado" na hora, não mede leituras fora do
    compasso e não vira taxa negativa quando o serviço reinicia (`ir-ui/src/trafego.rs`).
- **Windows:**
  - `cargo xtask check`, `cargo clippy --workspace --all-targets -D warnings` e `cargo fmt` limpos;
  - `cargo test --workspace`: 1 328 passando, nenhuma falha.
- **Linux, num Fedora 44 em contêiner:** `clippy -D warnings` limpo; 1 344 testes passando, nenhuma
  falha.
- A janela de verdade, na demonstração (`--simulado --pareado`), fotografada:
  - movendo: "↓ 2,2 MB/s ↑ 15,1 KB/s", em azul;
  - parado: "parado", em cinza;
  - Preferências com o cartão "Onde fica o outro computador" logo depois de "Quem pode controlar".

  A primeira foto mostrou a linha do tráfego entre "Conexão" e a frase que explica a conexão; ela
  foi para baixo da frase.
- **Não verificado ainda:** a linha com o serviço de verdade, numa cópia e numa sincronia entre as
  duas máquinas. A contagem tem teste com socket de verdade, mas a tela só foi vista na
  demonstração.
