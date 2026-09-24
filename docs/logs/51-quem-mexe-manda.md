# Quem mexe, manda: os dois teclados controlam o outro

**Data:** 2026-09-24

**Itens:** [ADR-0014](../adr/0014-controle-simetrico.md) — controle simétrico; protocolo 6.

**O que foi pedido:** que qualquer um dos dois teclados e mouses controle o outro computador, sem
ninguém ser "o dono do teclado", do jeito mais natural possível.

## O que mudou para quem usa

- **Não há mais papel.** De qualquer computador, encostar na borda leva o cursor e o teclado ao
  outro.
- **Quem mexe por último, manda.** Mexer no mouse ou no teclado do computador que está sendo usado
  de longe traz o controle de volta a ele na hora. A tela desse computador ensina o gesto: *"fedora
  está usando este computador. Mexa no mouse daqui para voltar a usá-lo."*
- **Contra tremida.** Um esbarrão não retoma: vale um clique, a roda, uma tecla que não seja só um
  modificador, ou 20 px de movimento em 300 ms; e nada retoma nos 150 ms depois de o outro chegar.
- **A posição do outro se escolhe dos dois lados**, na tela inicial. O outro passa à oposta sozinho
  e conta: *"O outro computador mudou de lado: agora ele fica à esquerda deste."*
- **"Papel deste computador" virou "Quem pode controlar"**: *Os dois* (padrão), *Só este*, *Só o
  outro*. Com *Só este* num computador, a borda do outro vira parede — o cursor não atravessa para
  ser devolvido.
- **Bloquear juntos** vale dos dois lados, e a faixa de impedimento diz qual sentido não funciona
  ("não consegue ler o próprio teclado, então só o outro controla este").

## Como ficou por dentro

- **`ir-session`:** `Phase::Engaged`, que queria dizer uma coisa em cada papel, virou
  `Phase::Sending` e `Phase::Receiving` — a direção está no tipo. `Role` virou `Policy` (quem pode
  controlar quem). `session/direction.rs` concentra as três regras: retomar, contra tremida, e os
  dois atravessando juntos (cede o `MachineId` maior). `server.rs`/`client.rs` viraram
  `sending.rs`/`receiving.rs`, as duas metades que toda máquina tem. A borda (`session/edge.rs`)
  passou a ser dos dois, com a escolha mais recente valendo, pelo horário.
- **Protocolo 6:** `Control::Reclaim` entrou, `Control::Role` saiu, `EdgeConfig` leva `chosen_at`, e
  `Capabilities::declines_control` diz no `Hello` se a ponta recusa ser controlada — derivado da
  política pela própria sessão, para as duas coisas nunca divergirem. A versão mínima subiu para 6
  (ver o ADR, §6).
- **Serviço:** `actor/politica.rs` (a política e a borda) e `actor/entrada_local.rs` (abrir captura
  e injeção, sempre as duas). No Linux, a condução do cursor ([log 50](50-o-cursor-que-o-servico-conduz.md))
  vale também enquanto o outro controla este; a ressincronização do cursor depois de uma sessão
  recriada saiu, porque puxaria o cursor de volta de onde o outro o deixou — quem recria a sessão
  leva o ponteiro junto.
- **Configuração:** `role` saiu do arquivo; um arquivo antigo sobe com `politica = "ambos"`.
  `papel_escolhido_em` virou `borda_escolhida_em`.
- **Tamanho:** a decisão de encarnação (a quais quadros pertencem à sessão) saiu do `ir-session` para
  `ir-confiabilidade::incarnation`, onde já moravam as sequências, e o crate voltou para baixo do
  teto; as frases do estado foram para `ir-ipc/src/status/frases.rs`.

## Um defeito que o teste achou antes da bancada

No primeiro rascunho, **qualquer** tecla retomava o controle. Em B, sendo usado por A, o Ctrl de
Ctrl+Alt+Shift+Espaço retomava; o resto do atalho, com B já em uso local, levava o controle de volta
para A. `the_switch_shortcut_here_takes_control_back` pegou. Um modificador sozinho não retoma mais:
é o começo de um atalho.

## Como foi verificado

- `ir-session/tests/symmetric.rs`, treze casos com duas sessões reais: o outro também atravessa;
  a volta pela borda oposta de quem está sendo usado; retomar pelo mouse, por clique e por tecla, com
  o que o outro segurava solto; tremida e modificador não retomam; a graça logo depois de chegar; o
  atalho retoma; os dois atravessando juntos terminam com exatamente um no controle; *Só este* é
  parede para o outro; *Só o outro* não atravessa; a queda solta o que o outro segurava.
- `ir-session/tests/edge.rs`, reescrito: bordas nunca escolhidas combinam pelo menor identificador;
  qualquer lado escolhe e o outro acompanha; a escolha mais recente vence; um anúncio mais velho não
  desfaz uma escolha mais nova.
- Vetores do protocolo 6 gravados (`hello` com a versão 6 e o campo novo, `edge_config`, `reclaim`).
- 1 097 testes no Windows, 1 040 no container do Fedora; clippy e `xtask` limpos.

## O que falta

A bancada, com o **MSI e o RPM novos nos dois** — um lado na versão 6 e outro na 5 recusam a
conexão com o motivo na tela. Atravessar e retomar dos dois lados, e conferir que nenhuma tecla fica
presa na troca.
