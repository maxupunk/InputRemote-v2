# O meio fixado de um lado só, e o rádio que ninguém religou

**Data:** 2026-11-25

**Itens:** desempenho do ponteiro em bancada (continuação do [log 79](79-o-mouse-que-andava-menos.md))

## O que se mediu

O log 79 corrigiu três causas no código, sem bancada. Aqui a bancada: Windows `SAMSUNG-MAXUEL` ×
Fedora `10.0.0.141`, ainda com os binários instalados antes do log 79.

**O caminho local do Linux é limpo.** Com o controle no Fedora, o serviço conduz o cursor (log 50):
lê o mouse, passa pela sessão e injeta pelo ponteiro virtual. Um mouse de teste por `uinput`
andou 400 passos de 2 unidades a cada 8 ms, e o ponteiro virtual do produto foi lido ao mesmo
tempo, com os carimbos do kernel: 400 de 400 movimentos viraram cursor, a cada 8,0 ms (p99
8,3 ms), com **0,3 ms** de atraso mediano do mouse ao cursor (pior caso 0,5 ms). O serviço do Linux
não é o gargalo.

**A medição de ponta a ponta Linux → Windows não serviu.** Lendo `GetCursorPos` no Windows a cada
1 ms enquanto o mouse de teste do Linux atravessava, o cursor voltava à borda a cada 0,2 a 0,5 s, e
o registro do Linux dizia `controle retomado sem atravessar here=false`: o Windows retomava o
controle, que é o que acontece quando o mouse **dele** se mexe (log 51). O mais provável é a pessoa
usando o Windows durante o teste, com o cursor dela pulando para o lado; não foi confirmado. Medir
esse sentido exige o Windows sem ninguém mexendo.

## O que os registros mostraram

1. **De manhã, 22 minutos só pelo Bluetooth.** O placar da rota dupla, que o Linux registra a cada
   minuto, dizia de 08:31 a 08:55 `rede ouvido há 842864 ms` até `1322864 ms`, com o Bluetooth
   ganhando tudo. Nada caiu: a rota estava viva pelo rádio, e a sessão não tinha regra para um
   portador calado. Tudo andou pelo Bluetooth, que é mais lento — o "mouse lento".
2. **O Windows estava com o meio fixado.** O estado do serviço do Windows dizia `portador:
   RedeLocal`, `motivo: FixadoPeloUsuario`. O Linux, no automático, discava o Bluetooth; o Windows
   aceitava o enlace e nunca mandava nada por ele, porque quem fixou exclui o outro. O Linux punha
   o rádio na rota e mandava **cada quadro pelos dois**: às 09:51 o placar dizia `Bluetooth 0` e
   `Bluetooth ouvido há 91845 ms`. A manhã parece o mesmo caso do outro lado — o meio fixado no
   Bluetooth —, mas isso não está provado.
3. **O rádio do Fedora estava bloqueado.** Depois do reinício das 09:04, `rfkill` dizia `Soft
   blocked: yes` e o `bluetoothctl`, `PowerState: off-blocked`. O Linux discava o Windows a cada
   rodada e ouvia "o par não atendeu no canal do InputRemote" — a mensagem culpa o outro
   computador, e o rádio parado era o daqui. O religamento automático (log 76) tinha falhado às
   09:01 e às 09:06 (`bluetoothctl power on:`). Reproduzir o religamento (desligar, 2 s, ligar) não
   bloqueou o `rfkill` de novo, então a causa do bloqueio fica em aberto, e o religamento não
   mudou. O rádio foi desbloqueado e ligado à mão, e o Windows voltou a vê-lo de pé.

## O que foi feito

- **O portador calado sai da rota, sem derrubar o enlace** (`Session::drop_mute_carrier`, a cada
  batida). Calado é três prazos de queda (3 s) sem nada por ele, com o outro falando
  (`Session::mute_carrier`). Derrubar e rediscar foi a primeira ideia, e foi descartada contra um par
  fixado: o enlace subiria de novo, o rádio voltaria à rota calado, e o vaivém se repetiria a cada
  poucos segundos.
- **A sonda que traz de volta** (`Session::probe_off_route`). Fora da rota, ninguém mandaria nada
  pelo portador, e três segundos sem Wi-Fi deixariam a sessão no Bluetooth até ela recomeçar. A cada
  *heartbeat*, quem não fixou portador manda um `AckOnly` sem sequência por todo portador de pé fora
  da rota; o primeiro quadro que chega por ele o devolve à rota, pelo despacho que já existia. Quem
  fixou não sonda, e a sonda de quem não fixou não convence quem fixou.
- **Duas peças saíram de crates no teto de linhas**, por fronteira, e não por espremer: o
  `PerInputCarrier<T>`, um valor por portador sem nada de sessão, foi para `ir_proto::carrier`, ao
  lado do `Carrier` (não vai para o fio; o `ir-session` o reexporta); e a versão do par guardada em
  disco foi do `ir-daemon` para `ir_configuracao::versao_do_par`, o crate do que a máquina guarda.

O formato no fio não mudou: o `AckOnly` sem sequência já existia, e um par antigo o recebe como
recebe qualquer confirmação pura.

**Arquivos:** `crates/ir-session/src/session/{route,link,consultas,state}.rs`,
`crates/ir-session/tests/route.rs`, `crates/ir-session/tests/route_silence.rs` (novo),
`crates/ir-proto/src/carrier.rs`, `crates/ir-configuracao/src/{lib,versao_do_par}.rs`,
`crates/ir-daemon/src/{arquivos.rs,actor/pastas.rs}`, `docs/03-protocolo.md` §2.1.

## Verificação

- `cargo test` de `ir-proto`, `ir-session`, `ir-confiabilidade`, `ir-configuracao`, `ir-servico`,
  `ir-daemon`, `ir-transporte`, `ir-net`, `ir-ipc` e `ir-painel`: tudo verde. Testes novos
  (`route_silence`): o portador que o par nunca usa sai da rota sem soltar nada e continua sondado;
  a sonda o devolve quando ele volta; contra um par fixado na rede o rádio sai e **não** volta, sem
  vaivém; com os dois calados nenhum é dado por calado, e quem cuida é o prazo de queda.
- `cargo clippy --workspace --all-targets` sem aviso; dos crates puros também para
  `x86_64-unknown-linux-gnu`; `cargo fmt --check` e `cargo xtask check-limits` ok.
- **Falta:** instalar nos dois lados e medir Linux → Windows com o Windows parado, e Windows →
  Linux lendo o ponteiro virtual do Fedora como acima.

## Decisões

- O meio fixado continua sendo de cada computador. Fixar de um lado só é legítimo — a pessoa pode
  não querer o rádio no notebook —, e agora o outro lado se adapta em três segundos em vez de mandar
  tudo dobrado para sempre.
