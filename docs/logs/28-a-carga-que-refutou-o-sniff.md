# A carga que refutou o *sniff*, e o rádio que ficava ocupado

**Data:** 2026-09-16

**Itens:** PoC-2 — medidor de RTT com carga de 125 msg/s. Uma hipótese derrubada e um defeito de
robustez achado na bancada.

## A hipótese, e por que ela caiu

Os logs [26](26-o-bluetooth-que-nao-existia.md) e [27](27-o-par-que-voltava-sozinho.md) deixaram
uma suspeita registrada: a latência alta viria do modo de economia do rádio. O raciocínio era
plausível — as sondas eram sequenciais, uma por vez, então o enlace ficava ocioso entre elas, e
um enlace ocioso entra em *sniff*, com intervalo de dezenas de milissegundos. Sob carga, dizia a
hipótese, o enlace não adormeceria e a medida melhoraria.

O medidor passou a emitir **125 sondas por segundo sem esperar resposta**, casando cada volta com
a ida pelo número de sequência. Resultado:

| | Sequencial (log 26) | Sob carga, 123/s |
|---|---:|---:|
| mediana | 49,84 ms | **51,21 ms** |
| p90 | 68,94 ms | 102,27 ms |
| p99 | 90,02 ms | **136,16 ms** |
| pior | 96,91 ms | 142,19 ms |

600 sondas enviadas, **600 respondidas, zero perdidas**.

**A hipótese está errada.** Se fosse *sniff*, o tráfego contínuo teria derrubado a mediana. Ela
ficou igual — 51,21 contra 49,84 ms — e a cauda **piorou**: o p99 quase dobrou.

## O que o formato da medida diz

Mediana estável com cauda crescente, e sem perda nenhuma, é a assinatura de **enfileiramento**, e
não de enlace adormecendo. Com ida e volta de ~51 ms e emissão a 125/s, há cerca de seis sondas
em voo o tempo todo; se o meio não escoa nesse ritmo, a fila cresce e o atraso extra aparece
inteiro na cauda, enquanto o caso mediano continua governado por um custo fixo por troca.

Duas explicações merecem investigação, e nenhuma delas foi verificada ainda:

1. **Custo fixo por quadro.** Cada sonda tem 8 bytes de conteúdo e sai num quadro próprio, com
   `flush` a cada um ([`canal.rs`](../../crates/ir-bt/src/canal.rs)). Num rádio cujo escalonamento
   é por intervalo, o que domina não é o tamanho, é a quantidade de trocas.
2. **A vazão do canal RFCOMM negociado.** A MTU efetiva das duas pilhas ainda é item aberto da
   PoC-2, e é ela que diz quantos quadros por segundo cabem.

Fica registrado o método: **uma hipótese que sobrevive a um teste que poderia derrubá-la vale
mais que uma que nunca foi testada** — e esta não sobreviveu.

## O rádio que ficava ocupado

No meio disto apareceu um defeito de robustez, e este atinge quem usa.

Depois de uma sessão encerrada **abruptamente** dos dois lados — o processo morto, sem fechar o
canal com jeito —, a conexão seguinte falhou:

```text
ligando para 74:13:EA:A6:5A:99 …
erro: erro de socket Bluetooth: Device or resource busy (os error 16)
```

O diagnóstico: o BlueZ ainda mostrava `Connected: yes` com o par. O enlace de baixo nível
sobrevive ao processo, e uma sessão RFCOMM meio aberta sobre ele faz o `connect` seguinte para o
mesmo canal responder ocupado. Derrubar o enlace no sistema
(`bluetoothctl disconnect`) resolveu **na primeira tentativa**, e a medição correu inteira depois
disso.

O sintoma para quem usa seria: fechou o programa, abriu de novo, e "não conecta" — até o sistema
expirar o enlace sozinho, o que leva um tempo indeterminado. O erro bruto do socket não ajudava
em nada.

Agora esse caso tem nome e instrução próprios ([`BtError::Ocupado`](../../crates/ir-bt/src/error.rs)),
como o ADR-0005 exige das causas que o usuário resolve.

## O que ainda não foi provado

- **A causa da latência.** As duas explicações acima seguem em aberto, e a MTU efetiva das duas
  pilhas — item da PoC-2 — é o próximo dado a buscar.
- **Latência adicionada**, medida como [01, §6](../01-visao-e-escopo.md) define: carimbo na
  captura contra a injeção. O número desta bancada é proxy de transporte.
- **Comparação lado a lado com UDP**, que é o que diz se 51 ms é do rádio ou do produto.
- Teclado e mouse atravessando, o socket na sessão 0, e Windows↔Windows.
