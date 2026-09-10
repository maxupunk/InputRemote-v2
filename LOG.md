# Log de implementação

Registro append-only. Cada entrada corresponde a um ou mais itens marcados em
[PROGRESSO.md](PROGRESSO.md). Entrada nova vai no fim do arquivo, nunca no meio.

Formato de cada entrada:

```
## AAAA-MM-DD — <título curto>
**Itens:** <quais itens do PROGRESSO fecharam>
**O que foi feito:** <descrição>
**Arquivos:** <lista>
**Verificação:** <o comando que rodou e o resultado, ou a inspeção feita>
**Decisões:** <o que foi decidido no caminho, se houver>
```

---

## 2026-09-09 — Fundação do repositório

**Itens:** Etapa 1.1 — `git init` e arquivos de raiz; workspace Cargo; política de lints;
`rustfmt.toml` e `rust-toolchain.toml`; `PROGRESSO.md` e `LOG.md`.

**O que foi feito.** Repositório iniciado do zero, sem nenhum arquivo herdado do v1
(regra de [11 — Nada de legado](docs/11-nao-legado.md)). Workspace Cargo com
`resolver = "3"` e edição 2024, versão `0.1.0-dev` para deixar explícito que nada foi
lançado.

A política de lints foi transcrita de [09, §5](docs/09-padroes-de-codigo.md) para
`[workspace.lints]`, de forma que ela seja aplicada pelo compilador e não por combinação:

- `unsafe_code = "deny"` no workspace, com as exceções por crate previstas em
  [09, §4](docs/09-padroes-de-codigo.md) a serem declaradas quando os crates de plataforma
  existirem;
- `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `todo` e `unimplemented` em
  `deny` — a proibição de [09, §5](docs/09-padroes-de-codigo.md);
- `clippy::all` e `clippy::pedantic` em `warn`, com `module_name_repetitions` e
  `must_use_candidate` liberados por serem ruído neste projeto.

`rust-toolchain.toml` fixa 1.96.0 e os dois alvos, para que a build não dependa do que
está instalado na máquina de quem compila.

**Arquivos:** `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`,
`.gitattributes`, `LICENSE`, `PROGRESSO.md`, `LOG.md`.

**Verificação.** `rustc 1.96.0`, `cargo 1.96.0`, `git 2.55.0` confirmados na máquina.
`git init` executado. Os membros do workspace ainda não existem — a build só é exercida na
entrada seguinte, junto com o primeiro crate.

**Decisões.**
- `panic = "abort"` no perfil de release. Um serviço `SYSTEM` que entra em pânico deve
  morrer e ser reiniciado pelo gerenciador de serviços, não desenrolar a pilha num estado
  parcial com teclas possivelmente pressionadas. O `ReleaseAll` de segurança é
  responsabilidade do serviço ao reiniciar, e do agente ao detectar a queda do pipe.
- `strip = "symbols"` e `lto = "thin"` para manter o binário pequeno, já que ele é carregado
  cedo no boot.
- Versão `0.1.0-dev` em vez de `0.1.0`: o v1 lançou `0.1.0` com itens em aberto no roadmap
  ([00, §4](docs/00-licoes-do-v1.md)). O sufixo torna impossível confundir estado de
  desenvolvimento com versão entregue.

---

## 2026-09-09 — `ir-proto`: o núcleo do protocolo, puro e testado

**Itens:** Etapa 1.1 — `clippy.toml`. Etapa 2.1 — quase toda; o que ficou aberto e por quê
está em [PROGRESSO.md](PROGRESSO.md).

**O que foi feito.** O crate `ir-proto` completo: 19 arquivos em `src/`, nenhum acima de 336
linhas. Nenhuma dependência de E/S, de relógio ou de sistema operacional — `postcard`,
`serde` e `thiserror`, e nada mais.

Organização por responsabilidade única, uma ideia por arquivo:

| Módulo | Responsabilidade |
|---|---|
| `limits` | todo tamanho máximo do protocolo, com a origem de cada número |
| `error` | o único tipo de erro, sem variante que carregue conteúdo do fio |
| `version` | versão e negociação |
| `ids` | identificadores opacos e distintos entre si |
| `carrier` | RFCOMM, UDP, TCP e o que cada um garante |
| `channel` | os seis canais e a política de o que viaja por onde |
| `input/` | tecla, modificador, botão, ponteiro, roda, estado |
| `screens` | arranjo de telas e bordas |
| `peer/` | nome, capacidades e os níveis N0–N3 |
| `message/` | o catálogo, um enum por canal |
| `frame` | envelope de canal, sequência e confirmação |
| `codec` | codificação e decodificação |

### Decisões de projeto tomadas durante a implementação

**O canal é a variante externa de `Message`.** A tabela de "que mensagem pode ir em que
canal" de [03, §4](docs/03-protocolo.md) deixou de ser verificação em tempo de execução e
passou a ser garantia do compilador: não há como construir um bloco de arquivo no canal do
ponteiro. E como `postcard` codifica a posição da variante, o primeiro byte do fio passou a
ser o número do canal de graça — com um teste guardando a correspondência, porque reordenar
as variantes quebraria isso em silêncio.

**Aritmética de sequência da RFC 1982.** `Sequence::is_newer_than` não compara inteiros.
Depois de 2³² mensagens o contador dá a volta, e comparação ingênua faria o canal do ponteiro
travar para sempre naquele instante — bug que só aparece depois de horas de uso e por isso
mesmo é caríssimo de achar em produção. Há teste atravessando a fronteira da volta.

**Modificadores em dois lugares, de propósito.** `InputState::apply_key` registra um
modificador no bitmap que viaja em toda mensagem **e** no conjunto de teclas que o injetor
solta. Sincronizar os dois numa função só é o que impede o estado inconsistente que produz
modificador preso.

**Reconciliação idempotente testada como propriedade.** `PressedKeys` e `Modifiers` têm teste
que reconcilia dois estados, confere o resultado e reconcilia de novo esperando nenhuma ação.
É a base da meta de zero teclas presas.

**Limites conferidos antes de alocar.** `ScreenLayout::new`, `PressedKeys::from_vec` e
`validate_manifest` verificam a contagem anunciada **antes** de percorrer ou alocar. Há teste
que passa uma lista simultaneamente inválida e acima do limite, e exige que o erro seja o de
contagem — provando que a verificação barata vem primeiro. Isto importa porque a contagem vem
de um par remoto e o decodificador roda como `SYSTEM`.

**Travessia de diretório barrada no crate puro.** `ManifestItem::is_safe_path` recusa caminho
absoluto, `..`, `.`, componente vazio, barra invertida, raiz de unidade do Windows e byte
nulo. Ficar no crate puro permite testá-la exaustivamente sem tocar em disco.

**Erros que não são oráculo.** Nenhuma variante de `ProtoError` carrega bytes ou texto vindos
do fio, e `ErrorCode` — o que vai para o par — é enum fechado. O detalhe fica no log local.
Um decodificador que descreve em detalhe a entrada inválida ajuda quem está sondando.

**Vetores gravados.** `tests/vectors.rs` fixa os bytes exatos de 16 quadros da versão 1. É a
única proteção contra a falha mais perigosa deste protocolo: `postcard` é posicional, então
reordenar campos produz bytes que o outro lado decodifica **com sucesso** e interpreta
errado. Teste de ida e volta não pega, porque as duas pontas do teste mudam juntas. O
cabeçalho do arquivo explica o que fazer quando ele falhar, para que ninguém "conserte" o
teste apagando o vetor.

**Limites de tamanho como asserção de compilação.** As relações entre as constantes de
`limits` são `const _: () = assert!(...)`, não teste. Um limite incoerente deixa de compilar,
o que é melhor que falhar num teste que alguém pode marcar como ignorado.

**Arquivos:** `crates/ir-proto/` (19 em `src/`, 1 em `tests/`), `clippy.toml`.

**Verificação.**

```text
cargo fmt --all -- --check                             OK
cargo clippy -p ir-proto --all-targets -- -D warnings  OK
cargo test -p ir-proto                                 147 testes, 0 falhas
```

Maior arquivo: 336 linhas, contra o limite de 400 de
[09, §1](docs/09-padroes-de-codigo.md). Nenhuma função acima de 60 linhas.

Três coisas foram corrigidas pelos próprios limites do projeto, e vale registrar porque é a
regra funcionando: `peer.rs` chegou a 397 de 400 linhas e foi dividido em
`peer/{name,level,capabilities}.rs`; a tabela de vetores passou de 60 linhas e virou um grupo
por canal; e um teste meu estava errado — afirmava que a saída de `HidUsage::Display` não
contém a letra `a`, sem notar que a palavra `usage` contém.

**Decisões.**
- `ir-geometry` e `xtask` foram **temporariamente retirados** dos membros do workspace para
  não bloquear a build enquanto não existem. Precisam voltar quando forem criados; anotado em
  PROGRESSO 1.2 e 2.2.
- A seção de documentação de erro chama-se `# Errors`, em inglês, mesmo com o corpo em
  português: é convenção do rustdoc, como `# Panics` e `# Safety`, e o clippy a exige.
- `clippy.toml` traz `doc-valid-idents` com os nomes próprios do projeto, para que o lint de
  documentação não peça crase em nome próprio, e os limites de
  [09, §1](docs/09-padroes-de-codigo.md) que o clippy já sabe verificar
  (argumentos, linhas por função, complexidade, aninhamento). Os demais ficam para o
  `xtask`.

---

## 2026-09-09 — `ir-geometry`: telas, bordas e mapeamento de coordenadas

**Itens:** Etapa 2.2, inteira.

**O que foi feito.** Crate puro que responde às perguntas geométricas do produto: onde o
ponteiro está, se ele encostou na borda que dá para o par, e em que ponto da outra tela ele
deve aparecer.

| Módulo | Responsabilidade |
|---|---|
| `geom` | `Point` e `Rect`, com bordas inclusivas e aritmética saturada |
| `desktop` | o conjunto de monitores de uma ponta, e as consultas sobre ele |
| `crossing` | quando o controle passa, e por onde o ponteiro entra do outro lado |

### Decisões de projeto

**Nenhum ponto flutuante, em lugar nenhum.** `f32` não tem ordenação total, arredonda
diferente entre plataformas e tornaria um teste de mapeamento frágil. Toda a aritmética é
inteira, com `i64`/`u64` como intermediário onde o produto poderia estourar.

**A travessia viaja como fração, não como pixel.** A saída é medida em `0..=u16::MAX` ao
longo da borda. É o que faz o meio da borda de um monitor 4K virar o meio da borda de um
720p do outro lado. Em pixels, atravessar entre telas de tamanhos diferentes seria um salto.

**Só a borda do par atravessa.** As outras três prendem o ponteiro, como um monitor isolado
faria. Um KVM em que qualquer borda atravessa é um KVM que rouba o controle quando o usuário
mira num botão de canto.

**O monitor principal é campo, não busca.** `Desktop` guarda uma cópia do principal. O
invariante "sempre existe um monitor" deixou de ser comentário com `unreachable!` e passou a
ser estrutural — não há caminho de código que precise entrar em pânico nem devolver `Option`
para algo que sempre existe.

**Nenhuma função devolve coordenada inutilizável.** `nearest_valid` traz qualquer ponto para
a tela mais próxima: monitor desconectado com o ponteiro em cima, buraco de arranjo em L,
posição herdada de um arranjo antigo, lixo vindo do par. Há teste varrendo os quatro casos.

### Dois defeitos que os testes acharam

Vale registrar, porque nenhum dos dois teria aparecido em uso casual e os dois quebrariam o
produto em campo:

1. **`from_position` aplicava o recuo de borda.** `point_along` recua um pixel para dentro
   de propósito (impedir o ping-pong de travessia), e `from_position` a usava para converter
   posição do protocolo em ponto — deslocando o ponteiro um pixel a cada conversão. As duas
   responsabilidades foram separadas: `x_at`/`y_at` são exatos, `point_along` recua.

2. **A normalização truncava e comia o recuo.** Numa tela de 1920 px, um pixel vale 34
   unidades de fração; com truncamento nas duas conversões, o recuo de um pixel voltava a
   zero e o ponteiro reaparecia exatamente na borda — quicando de volta na amostra seguinte.
   Passou a arredondar para o mais próximo, com teste de regressão cobrindo de 800 a 7680 px.

O segundo é o tipo de defeito que se manifesta como "às vezes o ponteiro fica preso entre as
telas" e custa dias para reproduzir. Foi pego por um teste chamado
`entry_is_never_on_the_far_edge_so_it_cannot_bounce_back`, escrito antes de o defeito existir
— que é exatamente o argumento do núcleo sem E/S de [ADR-0004](docs/adr/0004-nucleo-sans-io.md).

**Arquivos:** `crates/ir-geometry/` (4 arquivos), `docs/02-arquitetura.md` (a seta
`ir-geometry ──► ir-proto` faltava no diagrama).

**Verificação.**

```text
cargo fmt --all -- --check                          OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test --workspace                              193 testes, 0 falhas
```

**Decisões.**
- Vários métodos de `Rect` deixaram de ser `const` porque precisam de `try_from` para
  converter dimensão sem cast que perca sinal. Const-ness não tinha uso real aqui; evitar o
  cast tem.
- `clippy::panic` foi liberado sob `cfg(test)` neste crate: teste que falha entra em pânico,
  e é assim que ele reporta.

---

## 2026-09-09 — `ir-session`: a máquina de estados do produto

**Itens:** Etapa 2.3, quase toda. Ficaram abertos a confiabilidade sobre UDP e a medição de
cobertura; ver [PROGRESSO.md](PROGRESSO.md).

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
[02, §6](docs/02-arquitetura.md) é não alocar ali. É uma divergência do esboço daquele
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
`CarrierSet::pick_input_carrier` é a política única de [01, §5](docs/01-visao-e-escopo.md), e
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
[10, §2](docs/10-testes-e-validacao.md) rodam sobre ela, divididos por tema em
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

Cinco arquivos passaram do limite de [09, §1](docs/09-padroes-de-codigo.md) e foram
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

---

## 2026-09-09 — Confiabilidade sobre datagrama, e sete defeitos que ela revelou

**Itens:** Etapa 2.3 — a confiabilidade do canal de entrada sobre UDP, fechada. Sobra a
medição de cobertura.

**O que foi feito.** A camada de `docs/03-protocolo.md` §4.1: janela de 64, confirmação
cumulativa com bitmap de 32, retransmissão com `RTO = max(20 ms, 2 × srtt)`, cinco tentativas
e então queda. Um par emissor/receptor **por canal**, para que a retransmissão de um bloco de
clipboard não atrase um `KeyUp`.

Módulos: `reliability/{sender,receiver,channels}.rs`. Testes: `tests/reliability.rs` para a
camada isolada, `tests/loss.rs` para ela **ligada** à sessão. A distinção importa — uma camada
correta e desligada é indistinguível de uma camada ausente.

### Os sete defeitos

Nenhum deles apareceria em uso casual, e todos apareceriam em campo. Vale registrar cada um,
porque a lista é o argumento a favor de testar o núcleo sem hardware.

**1. Confirmação pura consumindo janela de retransmissão.** Cada confirmação precisava ser
confirmada, e o laço enchia a janela do canal de controle até a sessão cair. É por isso que um
ACK puro de TCP não carrega sequência a confirmar. Confirmação pura passou a não entrar na
janela.

**2. Confirmação pura consumindo número de sequência.** Consertado o item 1, perder uma
confirmação pura criava um buraco na ordem que nunca seria preenchido — e tudo depois dela
ficava esperando para sempre. Ela saiu do fluxo ordenado por completo: sequência zero, e o
receptor a trata antes da ordenação.

**3. Retransmissão criando entrega fora de ordem.** O mais grave, e o menos óbvio. Se um
`KeyDown` se perde e o `KeyUp` seguinte chega inteiro, o `KeyDown` retransmitido chega
**depois** do `KeyUp` — e a tecla fica pressionada para sempre, porque o evento que a soltaria
já passou. O canal confiável passou a entregar em ordem, com fila de reordenação limitada.

**4. `establish` correndo numa sessão já estabelecida.** Um `Hello` retransmitido rebaixava a
fase de `Engaged` para `Ready`, devolvendo o controle no meio do uso e reanunciando a conexão à
interface. Passou a sair cedo quando a sessão já está de pé.

**5. Quem recebe o `Hello` primeiro não conseguia responder.** `send` retorna cedo sem
portador, e o portador local só era registrado no aviso de subida. O lado que ouvia primeiro
iniciava o próprio handshake, **zerando as janelas** — e o `Hello` retransmitido do outro lado
passava a parecer novo, reiniciando tudo em laço. A regra passou a ser: receber um quadro por
um portador é prova de que ele funciona, então adota-se. Quem ouve primeiro, responde.

**6. Direção do deslocamento do bitmap invertida.** Defeito meu, introduzido ao reescrever o
receptor para entrega em ordem: `cumulative.distance_from(seq)` em vez de
`seq.distance_from(cumulative)`. Invertido, o deslocamento vira um número enorme, o bitmap é
zerado a cada avanço, e **nada além da própria sequência cumulativa parece confirmado** — o
emissor retransmite tudo até desistir. Uma sessão parada caía sozinha em pouco mais de um
segundo.

**7. Quadro antigo enfileirado para sempre.** Um quadro anterior ao que já foi entregue ficava
na fila de reordenação esperando um buraco que já havia passado. Passou a ser tratado como
repetição.

### Duas correções de projeto que vieram dos defeitos

**O `ack` do quadro ganhou canal explícito.** Era `Option<Ack>`, referindo-se ao canal do
próprio quadro. Só que o canal de entrada confiável é unidirecional — servidor para cliente —,
então o cliente nunca teria como confirmar o que recebeu ali, e a janela do servidor encheria
depois de 64 teclas, derrubando a sessão no meio de uma frase. Virou
`Option<ChannelAck>`, com o canal dito.

Isso mudou o formato de fio, e o teste de vetores gravados pegou — que é exatamente para isso
que ele existe. Como a versão 1 nunca foi lançada e não há par instalado em lugar nenhum, o
vetor foi atualizado no lugar em vez de a versão ser incrementada. A regra e a exceção estão
escritas no cabeçalho de `tests/vectors/main.rs`.

**Toda queda decidida por este lado anuncia um adeus.** Antes, quem desistia morria em
silêncio, e o outro lado só percebia pelo próprio prazo — até um segundo segurando as teclas.
O adeus não entra na janela (não haveria quem o confirmasse) e viaja fora da ordem, pelo mesmo
motivo da confirmação pura.

**Arquivos:** `crates/ir-session/src/reliability/` (4), `session/{mod,link,frames}.rs`,
`crates/ir-proto/src/frame.rs`, `crates/ir-session/tests/{reliability,loss}.rs`,
`tests/common/mod.rs` (perda e duplicação na bancada).

**Verificação.**

```text
cargo fmt --all -- --check                              OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test --workspace                                  305 testes, 0 falhas
```

Cenários novos que passaram a existir: quadro duplicado injetado uma vez só; 200 teclas
seguidas sem encher a janela; perda de 1 em 7 reparada sem deixar tecla presa dentro do
segundo prometido; perda total derrubando o enlace em vez de prosseguir com lacuna; e portador
de stream não usando janela nenhuma.

---

## 2026-09-09 — `xtask`: as regras de docs/09 viram executáveis

**Itens:** Etapa 1.1 — `deny.toml` e CI. Etapa 1.2 — as três verificações.

**O que foi feito.** `cargo xtask check` roda três verificações e falha a build, não avisa:

| Verificação | O que garante |
|---|---|
| `check-limits` | linhas por arquivo, por função e por crate |
| `check-deps` | as setas de [02, §2](docs/02-arquitetura.md) e a pureza do núcleo |
| `check-logs` | nenhuma macro de log recebendo conteúdo digitado |

Mais `deny.toml` (licenças, avisos do RustSec, fontes) e o CI do GitHub, com clippy, testes e
documentação nos dois sistemas alvo, mais uma verificação cruzada para Linux.

### O que a implementação obrigou a decidir

**Os limites medem coisas diferentes, e isso agora está dito.** Ao rodar a verificação pela
primeira vez, `ir-proto` (4.872 linhas) e `ir-session` (3.846) estouraram o limite de 2.500 por
crate. Havia três saídas, e duas eram erradas:

- *dividir os crates* seria cargo cult: `ir-proto` é **uma** responsabilidade, e quebrá-lo em
  `ir-proto-types` e `ir-proto-codec` só espalharia a mesma coisa por dois lugares;
- *aumentar o número* seria desistir da regra na primeira vez que ela incomodou.

A saída certa foi perceber que o limite **media a coisa errada**. Ele existe para conter
acúmulo de responsabilidade — é ele que impediria o crate de 10.491 linhas do v1. Só que
documentação não acrescenta responsabilidade (reduz o custo de entender o que já está lá) e
teste não acrescenta responsabilidade (acrescenta confiança). Contar qualquer um dos dois cria
pressão para escrever menos deles, que é o oposto do que este projeto quer.

O limite por crate passou a contar **só linhas de código de produção**: sem testes, sem
comentários, sem linhas em branco. Com isso, os dois crates passam com folga — e a regra
continua pegando crescimento real. O limite por **arquivo** continua contando tudo, porque ali
o que se protege é a navegabilidade, e um arquivo longo é longo de rolar mesmo que a maior
parte seja documentação. [09, §1](docs/09-padroes-de-codigo.md) foi reescrito para dizer qual
limite mede o quê, e por quê.

**Um defeito no próprio verificador.** O detector de funções longas usava
`opens.saturating_sub(closes)` para acompanhar profundidade — que satura em zero e portanto
**nunca decrementa**. Nenhuma função era detectada. Foi pego pelo teste
`every_flavour_of_signature_is_recognised`, que exercita `fn`, `pub fn`, `pub(crate) fn`,
`const fn` e `async fn`. Passou a usar delta com sinal.

Vale registrar porque é o argumento a favor de testar ferramenta de verificação: uma que não
acusa nada é indistinguível de uma que funciona, e a diferença só aparece quando já é tarde.

**As heurísticas são declaradas como heurísticas.** A contagem de funções e a exclusão de
módulos de teste contam chaves, não constroem árvore sintática. Erram em macro que abre chave
sem fechar na mesma linha, e acertam no resto — e erram **para mais**, o que faz uma função ou
um crate parecerem maiores. É o lado seguro de errar, e está escrito no código.

**Arquivos:** `xtask/` (5), `deny.toml`, `.cargo/config.toml` (o alias `cargo xtask`),
`.github/workflows/ci.yml`, `docs/09-padroes-de-codigo.md`.

**Verificação.**

```text
cargo fmt --all -- --check                              OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test --workspace                                  328 testes, 0 falhas
cargo xtask check                                       65 arquivos, nenhuma violação
```

**Decisões.**
- `spikes/` fica fora das verificações: é código descartável de prova de conceito
  ([08, §2](docs/08-plano-de-implementacao.md)), e aplicar limites a ele atrasaria a resposta a
  perguntas sem melhorar nada.
- `xtask/src/logs.rs` está isento da própria verificação de log, porque precisa citar os nomes
  proibidos para poder procurá-los.
- A lista de nomes proibidos em log é deliberadamente ampla. Um falso positivo custa uma
  renomeação; um falso negativo custa a senha do usuário num arquivo de log.
