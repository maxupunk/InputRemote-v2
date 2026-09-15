# A sessão que reiniciava a cada 200 ms

**Data:** 2026-09-15

**Itens:** Etapa 2 — confiabilidade sobre datagrama (defeito encontrado no teste físico). Mudança de
formato de fio: `Frame` ganha `epoch`.

## O sintoma

Com a janela destravada ([log 21](21-a-janela-que-travava-no-windows.md)), o pareamento fechou:
cada lado gravou a chave do outro. E a janela passou a alternar sem parar entre "Conectando…" e
"Desconectado — computador pareado está pareado, mas não está por perto".

Os registros dos dois serviços diziam a mesma coisa:

- às 18:23:21 UTC, `par gravado` e **`sessão estabelecida`** nos dois lados;
- às 18:23:22.99, 1,7 s depois, a primeira `sessão encerrada reason=Timeout will_retry=true`;
- daí em diante, **um encerramento a cada ~200 ms, nos dois lados, sem parar** — 1.452 somados
  em cinco minutos, sem nenhum erro de criptografia no meio.

## Por que o ritmo não fechava

O prazo de queda da sessão é de **1000 ms**, e ela caía **cinco vezes por segundo**. Não era
silêncio do par. E o serviço só tenta reiniciar a sessão a cada ~3 s (`RECONNECT_TICKS`), então o
reinício em 200 ms vinha de dentro da própria sessão.

Duas linhas do registro do Windows entregaram o resto. Às 18:23:30.843, `sessão estabelecida` e
`sessão encerrada reason=PeerClosed(Timeout)` saíram **no mesmo milissegundo**: um adeus que não
era desta sessão tinha acabado de matá-la.

## A causa

Nada no quadro dizia a qual sessão ele pertencia (`Frame = { message, seq, ack }`). Três pontos do
código se combinavam num laço:

1. **Quem acabou de zerar se ancorava no primeiro quadro que chegasse.** No receptor,
   `None => seq`. Esse primeiro quadro podia ser um `Ping` velho do par, de número 57, da sessão
   que tinha acabado de cair.
2. **Um quadro velho ressuscitava a sessão desligada.** `adopt_carrier_if_needed` passava a
   `Handshaking` ao receber *qualquer* quadro, sem mandar `Hello` e sem conferir de onde ele vinha.
3. **O `Hello` novo do par chegava com número 1**, parecia mais velho que a âncora 57, e era
   descartado calado como repetição. Sem `HelloAck`, o par retransmitia cinco vezes em
   ~100–200 ms, desistia com `Timeout`, zerava — e produzia uma nova leva de quadros "velhos" para
   o outro lado.

O gatilho da primeira queda foi o Wi-Fi do notebook, com economia de energia ligada: picos de
124 ms no sentido Windows → notebook, maiores que o orçamento de retransmissão.

O mesmo buraco tinha uma consequência pior do que a sessão caindo: um `KeyDown` velho, guardado na
fila de reordenação, podia ser **entregue na sessão nova como tecla digitada agora**.

Nenhum ajuste de prazo resolveria isso. Aumentar o número de retransmissões só adiaria a primeira
queda; depois dela, o laço seria o mesmo.

A hipótese da ancoragem em numeração velha e a medição do Wi-Fi vieram de outra sessão de trabalho
que conduzia o teste físico no mesmo repositório. O que se acrescentou aqui foram a cadência do
reinício do serviço, que tirou o serviço da lista de suspeitos, e o adeus velho no mesmo
milissegundo.

## A prova, antes de corrigir

`crates/ir-session/tests/incarnation.rs`, na bancada de duas sessões em memória, sem mudar nada
nela: `commands(lado)` guarda até os quadros que o roteador não entregou, então dá para capturar um
quadro e injetá-lo depois.

Três testes reproduzem o defeito, e **falhavam** no código de antes, cada um na asserção esperada:

| Teste | Falhava com |
|---|---|
| `a_stale_frame_does_not_anchor_the_next_session` | "o Hello da sessão nova precisa ser aceito mesmo depois de um quadro da anterior" |
| `a_peer_that_restarted_without_saying_goodbye_is_recognized` | "o servidor precisa reconhecer uma sessão nova do par, mesmo sem ter visto a antiga acabar" |
| `a_farewell_from_the_previous_session_does_not_end_the_current_one` | "um adeus da sessão anterior, chegando atrasado, não pode encerrar a atual" |

Dois outros protegem o que já funcionava, e passavam antes e depois: um `Hello` retransmitido não
reinicia uma sessão de pé, e um `Hello` de uma encarnação anterior também não.

## A correção

**Cada quadro carrega a época da sessão de quem o enviou.** `Frame` ganhou `epoch: Epoch` **no fim**,
e o primeiro byte continua sendo o canal. A cada aperto de mão começa uma encarnação nova, com
época nova. Quem recebe decide antes de qualquer outra coisa — prova de vida, confirmação,
ordenação, adoção de portador:

| O quadro é | E a época é | Então |
|---|---|---|
| qualquer um | a da sessão corrente do par | segue o caminho normal |
| `Hello` ou `HelloAck` | nova | o par começou outra sessão; se havia uma de pé, ela é encerrada soltando tudo (`LinkDown::PeerRestarted`), e esta ponta recomeça junto |
| `Hello` ou `HelloAck` | uma das 4 últimas aposentadas | eco atrasado de uma sessão que acabou; descartado |
| qualquer outro | diferente da corrente | resto de outra sessão; descartado |

Só um aperto de mão pode inaugurar uma encarnação. Um `Ping` velho não ressuscita uma sessão
desligada, um `Bye` velho não derruba a nova, e um `KeyDown` velho nunca vira tecla digitada.

As peças:

- **`ir-proto`**: `Epoch` e o campo no `Frame`, com `Frame::in_epoch`;
- **`ir-session`**: o módulo `session/incarnation.rs`, com a regra de admissão; a época carimbada
  nos três pontos que montam quadro — envio normal, confirmação pura e adeus; `LinkDown::PeerRestarted`,
  que encerra sem mandar adeus, porque o par já está em outra sessão;
  `ReliableChannels::reset_receivers`, para trocar a encarnação do par no meio do nosso aperto de
  mão sem perder o que nós já mandamos;
- **`ir-daemon`**: `SessionConfig::incarnation_seed` sorteada a cada sessão criada, inclusive na
  recriada por troca de papel ou de borda. O núcleo da sessão não sorteia nada (ADR-0004), e uma
  semente repetida entre duas execuções do serviço faria o par tomar a sessão nova pela antiga —
  exatamente o laço que a época existe para impedir;
- **`docs/03-protocolo.md`**: §4.3, "Encarnações de sessão".

## Mudança de formato de fio

É quebra de compatibilidade: **um lado com a versão antiga não decodifica o quadro novo.** Windows
e Linux precisam ser atualizados juntos.

Vale a exceção de pré-lançamento de `crates/ir-proto/tests/vectors/main.rs` — a mesma usada quando
entrou o `ChannelAck`: não há par instalado em lugar nenhum além desta bancada. Por isso os 16
vetores foram atualizados **no lugar** (cada um ganhou `00`, a época zero, no fim), e
`protocol_version` continua 1. Entrou um vetor novo que grava a época em varint:

| Vetor | Quadro | Bytes |
|---|---|---|
| `epoch` | `Ping { stamp_micros: 1 }`, seq 17, época `0x1234_5678` | `0007011100f8acd19101` |

A época aleatória ocupa até 5 bytes. O maior quadro do caminho quente, no pior caso, fica em
~35 bytes, contra o teto de 64; o teste de orçamento passou a usar `Epoch(u32::MAX)` para
continuar honesto.

## Arquivos

`crates/ir-proto/src/{frame,lib,codec}.rs`, `crates/ir-proto/tests/vectors/table.rs`,
`crates/ir-session/src/{config.rs,event/input.rs,reliability/channels.rs}`,
`crates/ir-session/src/session/{mod,link,frames,incarnation}.rs`,
`crates/ir-session/tests/incarnation.rs`, `crates/ir-daemon/{Cargo.toml,src/actor/papel.rs}`,
`docs/03-protocolo.md`.

## Verificação

- os três testes do defeito falhavam antes da correção e passam depois; os dois de proteção passam
  nos dois momentos;
- `Incarnations` tem testes próprios: épocas distintas mesmo dando a volta no `u32`, só aperto de
  mão inaugura encarnação, época aposentada continua aposentada, e a memória de aposentadas é
  limitada e esquece a mais antiga;
- os vetores gravados conferem, incluindo o novo;
- **469 testes** no workspace, nenhuma falha; `cargo clippy --workspace --all-targets`,
  `cargo fmt --check` e `cargo xtask check` limpos (137 arquivos; `link.rs` com 387 linhas).

## O que continua aberto

1. **Instalar nas duas máquinas.** Por ser mudança de fio, o MSI e o RPM precisam sair desta
   versão e entrar juntos. Até lá, a bancada continua com o laço.
2. **A borda está incoerente**: o Windows está com `bottom` e o Linux com `left`. Depende de onde o
   notebook fica fisicamente em relação à tela do Windows.
3. **O Wi-Fi do notebook com economia de energia** gera picos de mais de 100 ms. Com a correção, um
   pico derruba e reergue a sessão em vez de travá-la num laço, mas continua derrubando.
4. **Bluetooth não existe ainda** (Etapa 7). O registro diz `carrier=udp why=FellBackToNetwork`
   porque a política já prefere Bluetooth, e só não há transporte para escolher. As duas máquinas
   já estão emparelhadas pelo sistema.
