# Confiabilidade sobre datagrama, e sete defeitos que ela revelou

**Data:** 2026-09-09

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
