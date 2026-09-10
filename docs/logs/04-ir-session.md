# `ir-session`: a máquina de estados do produto

**Data:** 2026-09-09

**Itens:** Etapa 2.3, quase toda. Ficaram abertos a confiabilidade sobre UDP e a medição de
cobertura; ver [PROGRESSO.md](../../PROGRESSO.md).

**O que foi feito.** O núcleo do produto, puro: nenhuma E/S, nenhum relógio lido, nenhum
`async`, nenhum `Arc<Mutex<...>>`. Entra um evento, sai uma lista de comandos.

| Módulo | Responsabilidade |
|---|---|
| `time` | `Timestamp` e `Millis` injetados |
| `config` | papel, borda do par, e os prazos com verificação de coerência |
| `event/` | `Input`, `Command`, `Notice`, `CommandBatch` |
| `phase` | as quatro fases e a tabela de transições |
| `sequences` | um contador por canal |
| `session/state` | identidade, par, conjunto de portadores, carimbos |
| `session/link` | subida, handshake, prazos, queda |
| `session/server` | captura, travessia, snapshot, coalescência |
| `session/client` | injeção, reconciliação, travessia de volta |
| `session/frames` | despacho do que chega, e a emergência |

### Decisões de projeto

**O relógio é parâmetro, não campo.** `Timestamp` é um `u64` opaco, e `step` o recebe. Não
se usa `std::time::Instant` porque ele não pode ser construído com um valor escolhido — o que
tornaria impossível escrever "às 12h34 o par sumiu" num teste. Um cenário de reconexão de
trinta segundos roda em microssegundos.

**`CommandBatch` é buffer de quem chama.** `step` recebe `&mut CommandBatch` em vez de
devolver um `Vec`. O caminho quente processa milhares de eventos por segundo, e a regra 2 de
[02, §6](../02-arquitetura.md) é não alocar ali. É uma divergência do esboço daquele
documento, que mostrava `-> CommandBatch`, e é deliberada.

**A tabela de transições é dado, não `match`.** `Phase::TRANSITIONS` é um array de dez pares.
A especificação é uma tabela; o código que a implementa deve ser uma tabela também. Os testes
verificam propriedades — "toda fase pode cair", "não há atalho de offline para sessão em uso",
"o handshake nunca entrega o controle" — em vez de repetir a lista.

**Prazos coerentes por construção.** `Timings::is_coherent` recusa configuração que se
contradiz: um *heartbeat* mais lento que o prazo de queda faria a sessão cair sozinha a cada
ciclo, e o sintoma seria "desconecta de vez em quando" — defeito que se persegue por semanas.
`Session::try_new` devolve erro em vez de aceitar.

**A política de portador vive num lugar só, e devolve o motivo.**
`CarrierSet::pick_input_carrier` é a política única de [01, §5](../01-visao-e-escopo.md), e
devolve `(Carrier, CarrierChoice)` — o "por quê" viaja junto com o "o quê", e chega à
interface. O v1 tinha três políticas de degradação, uma por modo, e por isso ninguém
conseguia prever o comportamento.

**O servidor não sabe onde o ponteiro remoto está.** Ele manda deltas crus; quem converte
para posição absoluta é o cliente, que é quem conhece o próprio arranjo de telas. Dois lados
mantendo a mesma coordenada acabariam discordando dela.

**`ReleaseAll` vem primeiro, sempre.** Em toda queda, troca de portador, perda de agente,
emergência e encerramento. O teste
`a_link_drop_with_three_keys_held_releases_before_anything_else` verifica a **posição** do
comando no lote, não só a presença: a ordem é contrato.

**Um caminho de liberação, não um por situação.** `hand_control_back` é chamado no retorno
normal, na emergência, na troca de portador e na queda. Um caminho por situação é como se
esquece de um deles.

**Modificadores alinhados a cada mensagem.** O bitmap declarado é comparado com o aplicado
antes de processar o evento, e a diferença é corrigida. Custa 1 byte por mensagem e elimina a
categoria inteira do modificador preso.

### A bancada de dois lados

`tests/common/mod.rs` monta duas sessões e roteia todo `Command::Send` de um lado como
`Input::Received` do outro, em cascata, com detecção de laço. Os 25 cenários de
[10, §2](../10-testes-e-validacao.md) rodam sobre ela, divididos por tema em
`tests/link.rs`, `tests/crossing.rs` e `tests/input_state.rs`.

É a prova prática do argumento do núcleo sem E/S: travessia entre resoluções diferentes,
queda com três teclas pressionadas, monitor desconectado no meio da sessão, troca de
Bluetooth para rede, agente perdido, snapshot divergente e o atalho de emergência — todos sem
rádio, sem rede e sem segundo computador, em menos de um centésimo de segundo.

**Arquivos:** `crates/ir-session/` (14 em `src/`, 4 em `tests/`), mais
`Desktop::to_layout` em `ir-geometry`.

**Verificação.**

```text
cargo fmt --all -- --check                              OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test --workspace                                  268 testes, 0 falhas
```

### A regra de 400 linhas em ação

Cinco arquivos passaram do limite de [09, §1](../09-padroes-de-codigo.md) e foram
divididos, em vez de o limite ser aumentado:

| Arquivo | Antes | Virou |
|---|---:|---|
| `ir-session/tests/scenarios.rs` | 627 | `link.rs`, `crossing.rs`, `input_state.rs` |
| `ir-geometry/src/geom.rs` | 515 | `geom/{point,rect,scale}.rs` |
| `ir-geometry/src/desktop.rs` | 471 | `desktop/{mod,mapping}.rs` |
| `ir-session/src/event.rs` | 445 | `event/{input,command,notice}.rs` |
| `ir-proto/tests/vectors.rs` | 405 | `vectors/{main,table}.rs` |

Nenhum arquivo do repositório passa de 400 linhas agora, e a divisão temática deixou os
testes mais fáceis de achar do que estavam.

**Decisões.**
- Dois lints do clippy se contradiziam em `Phase::can_move_to`: `match_same_arms` queria
  fundir os braços, `unnested_or_patterns` queria separá-los. Expressar a tabela como array
  de dados resolveu os dois e ficou mais claro que qualquer uma das versões com `match`.
- `Timestamp::since` e `plus` deixaram de ser `const` porque `u64::from` e `u32::try_from`
  ainda não são utilizáveis em contexto constante, e preferir a conversão sem perda a um
  `as` é o que a política de lints exige. Nenhum uso em contexto constante foi perdido.
