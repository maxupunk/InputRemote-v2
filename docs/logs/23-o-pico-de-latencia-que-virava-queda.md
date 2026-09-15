# O pico de latência que virava queda

**Data:** 2026-09-15

**Itens:** Etapa 2 — confiabilidade sobre datagrama. Requisito da bancada: um pico de latência vira
atraso, e não queda.

## O pedido

Com o laço de reinício corrigido ([log 22](22-a-sessao-que-reiniciava-a-cada-200-ms.md)), ficou a
pergunta sobre o Wi-Fi do notebook, que tem economia de energia ligada e picos de 124 ms. A
resposta de quem usa foi direta: *no sistema antigo funciona assim, a latência só aumenta um pouco,
e não deveria cair nem desconectar por conta disso.*

Tinha razão. Desligar a economia de energia esconderia o defeito em vez de corrigi-lo, e por isso
ela continua ligada na bancada: o teste físico precisa provar que um pico não derruba a sessão.

## A causa

O emissor desistia de uma mensagem depois de **5 tentativas**, com prazo de retransmissão
`RTO = max(20 ms, 2 × srtt)`. Dois problemas somados:

- **Não havia espera crescente entre tentativas.** Numa rede local rápida o RTO fica em 20–40 ms,
  então as cinco tentativas se esgotavam em **100–200 ms** — menos que um pico do Wi-Fi.
- **O RTO não aprendia com os picos.** Pela regra de Karn, só entram na média as mensagens que não
  foram retransmitidas. Justamente as que atrasaram no pico são as que retransmitem, então a média
  continuava baixa e o RTO continuava curto no pico seguinte.

Um pico de 124 ms era tratado como "o par morreu". E isso não era engano do código: era o que a
§4.1 de [docs/03](../03-protocolo.md) prescrevia — "no máximo 5 tentativas; esgotadas as
tentativas, o enlace é declarado caído".

A intenção da regra era boa: não seguir com uma lacuna no canal de teclado, porque um `KeyUp`
perdido é uma tecla presa. Mas **esperar mais não cria lacuna**. A mensagem continua na fila, na
ordem, e só chega mais tarde. O que precisa derrubar a sessão é o par sumir de verdade.

## A prova, antes de corrigir

Quatro testes novos, rodados contra o emissor antigo, falharam com mensagens precisas:

| Teste | Falhou com |
|---|---|
| `the_link_is_dropped_only_when_a_message_outlives_the_deadline` | "desistiu aos **100 ms**, antes do prazo de 1000 ms" |
| `retransmissions_back_off_instead_of_hammering_the_link` | reenvios em `[20, 40, 60, 80]`, de 20 em 20 |
| `a_stall_shorter_than_the_deadline_is_waited_out` | um silêncio de 400 ms virou desistência |
| `a_stall_shorter_than_the_promised_second_does_not_drop_the_session` | "o servidor derrubou a sessão por um pico de 400 ms" |

O último é o de sessão inteira: um par em uso, com tecla pressionada, fica 400 ms sem entregar nada
em nenhum sentido, e depois volta ao normal por 2 s. Ninguém pode cair, e a tecla que atravessou o
pico continua coerente dos dois lados.

## Um furo no meu próprio teste

A primeira versão do teste de espera fixava **no máximo 5 envios**, com reenvios aos 20, 60, 140 e
300 ms. Ao desenhar a implementação ficou claro que isso continuaria errado. Num travamento de
400 ms, as cinco cópias de uma mensagem mandada no começo se perdem todas, e ela nunca mais é
reenviada — a sessão cairia do mesmo jeito, só que aos 1000 ms em vez de 100 ms.

O teste passou a exigir o comportamento certo: **continuar reenviando até o prazo**, com a espera
dobrando até um teto. O reenvio que sai depois do pico é o que salva a mensagem.

## A correção

- **Espera dobrando a cada reenvio**: `rto`, `2 × rto`, `4 × rto`…, com teto de um quarto do prazo
  de queda, e nunca menos que o próprio `rto`. No piso, os reenvios saem aos **20, 60, 140, 300,
  550 e 800 ms**.
- **Desistir por tempo, não por contagem**: cada mensagem guarda quando saiu pela **primeira** vez,
  além da última. O enlace cai só quando uma mensagem passa de **1 s** sem confirmação, contado do
  primeiro envio — o mesmo prazo de queda que [docs/01](../01-visao-e-escopo.md) §6 já promete.
- **`max_retransmits` saiu** de `Timings`, de `Sender::on_tick` e de `ReliableChannels::on_tick`.
  Sem contagem de tentativas, ele não fazia mais nada, e uma configuração que não faz nada é pior
  que nenhuma: alguém ajustaria o número achando que muda algo.
- **A regra de coerência dos prazos** passou a exigir que caibam ao menos quatro reenvios no piso
  antes da queda (`min_retransmit × 4 ≤ link_timeout`), e que o piso não seja zero.
- **[docs/03](../03-protocolo.md) §4.1** descreve a regra nova e o porquê.

A garantia contra tecla presa continua de pé, por outro caminho. Perda de verdade ainda derruba a
sessão dentro do prazo prometido: `total_loss_drops_the_link_instead_of_going_on_with_a_gap` e
`the_link_falls_by_timeout_within_the_promised_second` passam sem alteração.

Não é mudança de formato de fio: só muda quando cada ponta reenvia e quando desiste. Mas, como a
época do log 22 é, as duas máquinas vão receber as duas correções juntas.

## O arquivo que passou do limite

Com os testes novos, `tests/reliability.rs` chegou a 445 linhas, e o `xtask` recusou. Em vez de
apertar a regra, o arquivo foi dividido pelo que ele testa:

- `tests/sender.rs` (15 testes): o emissor — janela, confirmação, retransmissão, espera e
  desistência;
- `tests/reliability.rs` (10 testes): o receptor — ordem, repetição, fila de reordenação, volta da
  numeração —, e o cenário de 5% de perda que exercita os dois lados juntos.

Na divisão, o comentário de documentação de um auxiliar ficou solto no fim do arquivo errado; o
compilador recusou, e ele voltou para o lugar.

## Arquivos

`crates/ir-session/src/reliability/{sender,channels}.rs`, `crates/ir-session/src/config.rs`,
`crates/ir-session/src/session/link.rs`,
`crates/ir-session/tests/{sender,reliability,loss}.rs`, `docs/03-protocolo.md`.

## Verificação

- os quatro testes novos falhavam no emissor antigo e passam no novo;
- os 25 testes de emissor e receptor continuam os mesmos depois da divisão (15 + 10);
- **472 testes** no workspace, nenhuma falha; `cargo clippy --workspace --all-targets`,
  `cargo fmt --check` e `cargo xtask check` limpos (138 arquivos).

## O que continua aberto

1. **Instalar nas duas máquinas**, junto com a época do log 22, e verificar no hardware **com a
   economia de energia do Wi-Fi ligada**: a sessão tem de ficar de pé pelos picos.
2. **Acertar a borda**: o notebook fica à esquerda do Windows, então o Windows passa a
   `peer_edge = "left"` e o Linux a `peer_edge = "right"`.
3. **Bluetooth** (Etapa 7), que é o portador preferido para teclado e mouse e ainda não existe.
