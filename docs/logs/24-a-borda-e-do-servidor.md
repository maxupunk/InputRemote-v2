# A borda é do servidor

**Data:** 2026-09-15

**Itens:** Etapa 6/interface: a borda de travessia. Defeitos encontrados no teste físico com as
correções dos logs 22 e 23 instaladas.

## A bancada, com as duas correções instaladas

O MSI e o RPM de `c76242c` entraram nas duas máquinas às 16:11 e 16:15, com a borda acertada à
posição física: o notebook fica à esquerda do Windows, então o Windows passou a `left` e o Linux a
`right`. A economia de energia do Wi-Fi do notebook ficou **ligada**, de propósito.

- **Sessão estabelecida** às 19:15:13 UTC, e 90 s de amostra sem nenhuma queda dos dois lados, com
  ping de 1,4 ms a 117,7 ms (média 9 ms, 0% de perda). Antes da correção, o mesmo Wi-Fi dava 1.452
  quedas em cinco minutos.
- **A travessia aconteceu**: às 19:21:43 os dois serviços registraram o controle indo para o
  notebook e voltando 0,6 s depois. É o primeiro registro de ida e volta no hardware.

Daí em diante apareceram dois problemas que não eram os dos logs 22 e 23.

## Primeiro problema: a borda escolhida dos dois lados

Entre 19:30 e 19:33 o usuário mexeu em "Onde fica o outro computador" nas duas janelas. Os registros:

| Hora (UTC) | Windows | Notebook |
|---|---|---|
| 19:30:39 | borda `right`; sessão encerrada `UserStopped` | sessão encerrada `PeerClosed(UserRequested)`, `will_retry=false` |
| 19:31:08 | sessão encerrada `PeerClosed(UserRequested)` | borda `left`; `UserStopped` |
| 19:32:23 | borda `left` | `PeerClosed(UserRequested)` |
| 19:32:27 | `PeerClosed(UserRequested)` | borda `right` |
| 19:32:51 | — | borda `left` |

As duas máquinas terminaram com `left`. Com o Windows em `left`, o notebook precisava de `right`
para devolver o controle pelo lado certo.

Dois defeitos juntos:

1. **Cada máquina tinha a própria borda.** Nada ligava a escolha de uma à da outra, e a janela das
   duas oferecia a escolha. O pedido de quem usa foi direto: *a fonte de verdade é o servidor; quando
   um tiver a direita, o outro fica à esquerda; e no cliente não deveria nem ter essa opção.*
2. **Trocar a borda refazia a sessão inteira** ([log 19](19-a-troca-que-vale-na-hora.md)). A sessão
   velha saía por `stop(UserStopped)`, que manda ao par `Bye(UserRequested)` — e esse motivo diz ao
   par para **não** reconectar. O par só voltava porque o outro lado reabria o aperto de mão. A
   janela passava por "Conectando", "Desconectado" e "Pronto" a cada clique.

O protocolo já tinha a mensagem certa: `Control::EdgeConfig { peer_edge }`, com vetor gravado. Só
que nenhum lado a mandava, e quem recebia ignorava, com o comentário "a borda vem das preferências
locais, não do par".

## A prova, antes de corrigir

`crates/ir-session/tests/edge.rs`, na bancada de duas sessões em memória, contra um esboço que
aceitava `Input::SetPeerEdge` sem fazer nada:

| Teste | Falhava com |
|---|---|
| `the_client_takes_the_opposite_of_the_servers_edge_when_the_session_starts` | as duas com `left`: o cliente continuava `Left`, e precisava de `Right` |
| `changing_the_edge_on_the_server_keeps_the_session_up` | a borda do servidor não mudava |
| `after_the_change_the_new_edge_is_the_one_that_crosses` | a borda antiga continuava atravessando |
| `changing_the_edge_while_controlling_the_client_hands_control_back_first` | o servidor seguia `Engaged` |

Três protegem o que precisa continuar valendo, e passavam nos dois momentos: o cliente que já
concorda não grava nada, o cliente não escolhe a borda, e o servidor ignora um `EdgeConfig` forjado
com a época e a numeração corretas da sessão.

## A correção

**Na sessão** (`ir-session`, módulo novo `session/edge.rs`):

- `Input::SetPeerEdge(edge)` — a escolha do usuário. Só vale no servidor. Se o controle está com o
  cliente, ele volta antes, pelo caminho da volta normal (`LeaveScreen` e soltar tudo), e só então a
  borda muda.
- O servidor manda `EdgeConfig` ao estabelecer a sessão e a cada troca, pelo canal de controle, que
  entrega em ordem: um `LeaveScreen` chega ao cliente antes da borda nova.
- O cliente, ao receber, passa a usar a **oposta**. Se estivesse controlado — o servidor não deixa
  isso acontecer, mas a volta depende da borda —, solta tudo antes.
- `Notice::EdgeChanged { edge }` sai sempre que a borda em uso muda, para o serviço gravar.

**No serviço** (`ir-daemon`):

- `trocar_borda` recusa no cliente com a falha nova `Falha::BordaDoServidor`, sem gravar nada. No
  servidor, grava e entrega `SetPeerEdge` à sessão **em uso**. A sessão não é mais refeita.
- `adotar_borda` grava a borda que o servidor anunciou. Sem isso, o cliente subiria com a borda
  velha na próxima partida e atravessaria errado até reconectar.

**No contrato e na janela:**

- `Falha::BordaDoServidor`, no fim do enum (índice 9), com o que fazer: *"Troque a borda no computador
  que tem o teclado e o mouse. Este acompanha sozinho, sem reconectar."*
- "Onde fica o outro computador" só aparece no servidor. O serviço simulado recusa o pedido no
  cliente, como o de verdade.
- [docs/03](../03-protocolo.md) §6 descreve quem manda `EdgeConfig` e o que o outro lado faz.

Não muda o formato de fio: `EdgeConfig` já existia. Mas um servidor antigo não manda a mensagem, então
as duas máquinas entram juntas de novo.

## Segundo problema: o notebook trocando de ponto de acesso

Antes de qualquer clique na borda, das 19:25:03 às 19:27:00, a sessão caiu e subiu sem parar. O
diário do sistema do notebook explica:

| Hora local | Wi-Fi do notebook | Sessão |
|---|---|---|
| 16:25:08 | perdeu o sinal do ponto `3a:f0:65:2f:f0:11` | cai em `Timeout` |
| 16:25:09 | conectou a `82:0c:43:26:60:00` (5300 MHz) | 35 s sem firmar, `Timeout` a cada 3 s |
| 16:25:48 | trocou para `b8:69:f4:9d:84:e6` | firma, e fica de pé por 32 s |
| 16:26:24 | voltou a `82:0c:43:26:60:00` | oscila por 35 s |
| 16:26:59 | voltou a `3a:f0:65:2f:f0:11` | firma, e fica de pé até os cliques na borda |
| 16:32:33 | perdeu o sinal de novo | cai, e às 19:35:43 o notebook recebe quatro `Hello` de uma vez |

Quatro trocas de ponto de acesso em dois minutos. **Os trechos ruins coincidem com o ponto
`82:0c:43:26:60:00`**, e nos outros dois a sessão se manteve. A chegada de quatro `Hello` no mesmo
milissegundo, três segundos de intervalo entre cada um, mostra silêncios de mais de dez segundos no
caminho, não um pico de latência.

Isso **não** está corrigido neste log. O prazo de queda de 1 s é a promessa de
[docs/01](../01-visao-e-escopo.md) §6 contra tecla presa, e nenhum ajuste de retransmissão atravessa
um silêncio de dez segundos. A decisão entre um prazo de queda maior e "soltar tudo em 1 s, mas
manter a sessão e retomar quando o par voltar" fica para quem usa.

## Arquivos

`crates/ir-session/src/session/{edge,mod,frames,link,server,consultas}.rs`,
`crates/ir-session/src/event/{input,notice}.rs`, `crates/ir-session/src/config.rs`,
`crates/ir-session/tests/{edge.rs,common/mod.rs}`, `crates/ir-proto/src/message/control.rs`,
`crates/ir-daemon/src/{commands.rs,actor/papel.rs,actor/pedidos.rs}`, `crates/ir-ipc/src/falha.rs`,
`crates/ir-ui/{ui/inicio.slint,src/simulado.rs,tests/interface.rs}`, `docs/03-protocolo.md`.

## Verificação

- os quatro testes do defeito falhavam contra o esboço e passam depois; os três de proteção passam
  nos dois momentos;
- no serviço: a troca no servidor não refaz a sessão (ela continua no aperto de mão), o cliente
  recusa sem gravar, e o cliente grava a borda anunciada;
- na janela: o cliente não escolhe a borda, e a falha diz onde escolher;
- **483 testes** no workspace, nenhuma falha (eram 472: 7 da borda na sessão, 3 no serviço, 1 na
  janela); `cargo clippy --workspace --all-targets`, `cargo fmt --check` e `cargo xtask check`
  limpos (140 arquivos).

## O que continua aberto

1. **Instalar nas duas máquinas** e ver no hardware: trocar a borda no Windows sem a sessão cair, e o
   notebook passando sozinho à oposta.
2. **A sessão sobreviver à troca de ponto de acesso do Wi-Fi**, o segundo problema acima. Precisa de
   decisão.
3. **Bluetooth** (Etapa 7), o portador preferido para teclado e mouse, que não sofre com a troca de
   ponto de acesso.
