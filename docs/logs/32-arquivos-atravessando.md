# Arquivos atravessando, e a fronteira que o limite de linhas cobrou

**Data:** 2026-09-17

**Itens:** Etapa 4 — TCP de dados, fechado. Etapa 8 — o motor ligado à porta; progresso e
cancelamento, em parte.

**O que foi feito:** o canal de arquivos deixou de ser peças e passou a ser função. Uma árvore
atravessou entre duas instâncias do serviço, por socket de verdade, com Noise de verdade, e voltou
byte a byte igual — nos dois sentidos.

## A medição

Duas instâncias na mesma máquina, com `IR_DATA_DIR`, `IR_CONTROL_ENDPOINT` e `IR_AGENT_ENDPOINT`
próprios, sem elevação. Portas 52626 e 52627.

```text
A  canal de arquivos no ar porta=52626
B  canal de arquivos no ar porta=52627
A  canal de arquivos estabelecido          8 ms depois de subir
B  canal de arquivos estabelecido

A  enviando arquivos itens=8 total=200047
B  recebendo arquivos total=200047
A  envio concluído bytes=200047            8 ms
B  transferência concluída bytes=200047
```

A conferência, por SHA-256 de cada arquivo:

```text
origem:  7 entradas          destino: 7 entradas
igual: anexos/fundo/nota.md            (22 B)
igual: anexos/planilha com espaco.bin  (200 000 B — quatro blocos)
igual: resumo.txt                      (25 B)
igual: vazio.dat                       (0 B)
TUDO IGUAL, byte a byte
```

E no sentido de volta, um arquivo solto de 70 000 B: `A8B02D1A…BC07` nas duas pontas.

O que isso cobre, e que os testes em memória não cobriam: o enquadramento `u32` sobre TCP real, o
handshake `IK` sobre socket real, a contagem implícita do contador do Noise ao longo de mais de
quarenta quadros, a pasta vazia sobrevivendo, o arquivo de zero byte sobrevivendo, e a publicação
sem nível a mais — `recebidos/relatorio de janeiro`, espelhando a origem.

Um arquivo solto chega como **arquivo**, e não como pasta com um arquivo dentro. Era o que o
[log 31](31-o-motor-de-transferencia.md) previa; aqui está visto.

## A fronteira que o limite de linhas cobrou

Ao ligar a transferência ao serviço, o `xtask` reprovou:

```text
crates/ir-daemon:2952: crate com 2952 linhas de produção em src/, limite 2500.
                       Extraia um crate quando a fronteira estiver comprovada.
```

Não era ruído. O `ir-daemon` estava no teto, e a transferência é justamente uma responsabilidade
nova: ela é o **único** lugar que conhece o motor (`ir-files`) e a porta (`ir-transporte`) ao mesmo
tempo — a mesma forma de fronteira que o `ir-transporte` desenha para os portadores de entrada.

Virou `ir-transferencia`. O limite fez o que existia para fazer: não pediu um número maior, pediu a
fronteira que estava faltando. Vale registrar que ela ficou **comprovada** antes de ser desenhada, e
não o contrário.

## O que o ator sabe da transferência

Um campo:

```rust
/// Por onde pedir um envio de arquivos. O ator encaminha e segue; não conduz nada.
pub(crate) arquivos: ir_transferencia::Pedidos,
```

Nada mais. Ele não vê bloco, não vê socket, não espera transferência — ele gira a cada 5 ms e uma
transferência leva minutos. A resposta ao pedido é "recebi"; o que acontece depois chega por
`Aviso::Transferencia`.

## Três decisões tomadas no caminho

**Quem disca, na prática.** A regra da chave maior do [ADR-0010](../adr/0010-canal-de-dados-em-tcp-proprio.md)
diz quem tenta primeiro. Mas ela sozinha travaria o caso em que **só** o lado de chave menor conhece
o endereço do outro: ele esperaria para sempre. Então o lado não preferido disca também, depois de
cinco segundos de carência. Os dois escutam sempre.

**Um `Mutex` no remetente, e não um segundo socket.** As respostas de quem recebe (`Accept`,
`Verified`) saem pelo mesmo socket por onde quem envia despeja blocos. A seção crítica é uma
escrita, sem decisão dentro, e as respostas são raras ao lado dos blocos.

**Mandar arquivo é `Configurar`, e não `Elevado`.** Exigir elevação seria exigir elevação **a cada
colagem**, já que é este o caminho que o Ctrl+C vai usar — e uma permissão que atrapalha o uso
normal acaba desligada. O portão da operação é o pareamento: existe um par, confirmado por código de
seis dígitos nas duas telas, e a permissão de arquivos é revogável só para ele.

## Uma aspereza que fica registrada

O canal de arquivos sobe com a chave fixada que existia **na subida**. Parear agora e esperar
transferir na mesma execução não funciona: é preciso reiniciar o serviço. A entrada não tem esse
problema — ela reconecta sozinha.

Apareceu na própria bancada, que teve de parear, descer e subir. A correção é um `watch` da chave do
par, e está no PROGRESSO como item aberto. Não está escondido atrás de um "funciona".

## Arquivos

- `crates/ir-transporte/src/dados.rs` (novo) — a segunda porta: `Porta`, `Remetente`,
  `Destinatario`, e 6 testes em loopback TCP real
- `crates/ir-transferencia/` (novo) — `lib`, `sessao`, `recebendo`, `enviando`
- `crates/ir-ipc/src/transferencia.rs` (novo) — o vocabulário que a tela desenha, sem tipo de fio
- `crates/ir-ipc/src/ui.rs` — `Pedido::EnviarArquivos`, `Aviso::Transferencia`
- `crates/ir-ipc/examples/controle.rs` — a ação `enviar`, com barra de progresso
- `crates/ir-daemon/src/arquivos.rs` (novo), `main.rs`, `config.rs` (a pasta de recebidos),
  `actor/{mod,partes,pedidos,bancada}.rs`
- `xtask/src/deps.rs`, `docs/02-arquitetura.md`, `PROGRESSO.md`

**Verificação:** `cargo test --workspace` verde; `clippy --all-targets` silencioso; `cargo xtask`
nos três critérios, 189 arquivos. E a bancada acima, que é a parte que nenhum teste substitui.

**O que ainda não foi provado:** o Ctrl+C e o Ctrl+V. Hoje o gatilho é um pedido pelo canal de
controle; falta o `ir-clip` ligado ao agente para o clipboard do sistema disparar a mesma coisa. E
os 5 GB com a entrada intacta continuam sendo medição entre duas máquinas, não nesta.
