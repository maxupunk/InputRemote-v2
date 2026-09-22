# A rota dupla: Bluetooth e rede ao mesmo tempo, e o ponteiro que andava o dobro

**Data:** 2026-09-22

**Itens:** Etapa 7 — Bluetooth; política única de portador ([01, §5](../01-visao-e-escopo.md)).
[ADR-0012](../adr/0012-rota-dupla.md).

**O que foi pedido:** menor latência possível no Bluetooth, que às vezes sofre interferência e
atrapalha a resposta. A ideia de quem usa: com o meio de conexão no automático, mandar cada comando
pelo Bluetooth **e** pela rede ao mesmo tempo, e descartar a cópia do outro lado.

## Onde estava o tempo

Antes de desenhar, o caminho do quadro foi lido de ponta a ponta. O que o produto põe no caminho
custa microssegundos: no Linux o RFCOMM é um socket do kernel direto; no Windows a ponte passa por
duas threads e dois canais; o enlace cifra e enquadra ~27 bytes. Os ~50 ms de ida e volta medidos
no [log 26](26-o-bluetooth-que-nao-existia.md) e no [log 28](28-a-carga-que-refutou-o-sniff.md) são
do rádio — *sniff*, convivência de 2,4 GHz e intervalo de consulta —, não do produto. A rota dupla
ataca o problema pelo lado que o produto controla: não depender de um rádio só.

## O desenho

**A cópia é descartada na sessão, e não no transporte.** Cada portador tem o seu aperto de mão
Noise: as duas cópias de um quadro são bytes cifrados diferentes, e só depois de decifrar se vê que
são a mesma mensagem, por `(época, canal, sequência)` — que são da sessão.

**A rota dupla é um datagrama.** Dois caminhos juntos duplicam e reordenam, que é a garantia do
UDP, e a sessão já tratava UDP: sequência, confirmação, retransmissão, reordenação e descarte de
repetição. Nenhum mecanismo de entrega novo. O que mudou foi a sessão passar a tratar **toda** rota
como datagrama, inclusive o RFCOMM sozinho — é isso que deixa um portador entrar e sair da rota sem
refazer a sessão, sem soltar teclas e sem encarnação nova.

No código, a `Route` (`Single(carrier)` | `Dual`) substituiu o `carrier: Option<Carrier>` da
sessão, e `send`, retransmissão, confirmação pura e adeus passam por um despacho só,
`dispatch_on_route`, que é o único lugar que sabe que um quadro pode sair duas vezes.

## O defeito que a rota dupla teria transformado em regra

O canal do ponteiro ([03, §4.2](../03-protocolo.md)) manda descartar amostra com sequência antiga.
O código não descartava: toda `Motion` que chegava era aplicada. Como o movimento é **relativo**,
uma amostra duplicada movia o cursor o dobro. Com um portador só isso exigia um datagrama duplicado
pela rede, e era raro. Com a rota dupla, **toda** amostra chega duas vezes.

O teste foi escrito antes do filtro, e depois provado contra o filtro desligado de propósito:
sem ele, um movimento de 40 px anda 80; com ele, 40.

## O pareamento que a rota dupla teria aberto

Com um código na tela, o serviço concluía o pareamento com **qualquer** enlace que subisse:
`pending_peer.take().is_some()`, sem conferir a chave nem o portador. Com um portador só isso
exigia outro computador discando no meio da comparação. Com a rota dupla, o próprio par disca o
segundo portador sozinho — e o enlace dele concluiria o pareamento, gravando a chave antes de o
usuário confirmar ([04, §3.2](../04-seguranca.md)).

Agora só o enlace do pareamento o conclui: o mesmo portador pelo qual o código veio, e a mesma
chave do código. Outro enlace nesse intervalo é recusado sem gravar nada, e a queda dele não
abandona o pareamento em curso — o que o `on_link_down` fazia com qualquer queda. Um teste antigo
chamava `on_pairing_code` direto, sem o endereço que a produção sempre anota junto com o código;
passou a entrar pelo `Fato::CodigoDePareamento`, que é o caminho real.

## Os endereços

Manter os dois enlaces exige o endereço do par em cada portador. O da rede a descoberta acha pelo
`MachineId`, que sai da chave fixada — não precisa de sessão. O do rádio a rede não tem como achar:
o par o conta numa mensagem nova, `Control::Reach`, ao estabelecer a sessão, e o serviço o grava em
`PinnedPeer.radio` para a próxima subida já discar o Bluetooth.

O endereço do próprio rádio é lido sem D-Bus e sem `unsafe` novo: no Windows por
`BluetoothGetRadioInfo`, dentro do `winsock.rs`; no Linux pelo nome do diretório do adaptador em
`/var/lib/bluetooth`. Conferido nas duas máquinas da bancada, sem parar nenhum serviço:

| Máquina | Como | Endereço lido |
|---|---|---|
| Windows | `cargo test -p ir-bt -- --ignored` (não abre o canal 23) | `74:13:EA:A6:5A:99` |
| Fedora 44 | `ls /var/lib/bluetooth` | `AC:50:DE:47:EB:28` (e um `mesh`, que não é endereço e é ignorado) |

O `/sys/class/bluetooth/hci0` do Fedora não tem arquivo de endereço — confirmado na bancada —, e é
por isso que a leitura é pelo armazenamento do BlueZ.

Os dois lados passam a saber o endereço do outro ao mesmo tempo e discam juntos. A regra de quem
disca (`turno`), que resolvia isso no UDP, saiu do `ir-net` para o `ir-crypto` e vale agora para o
rádio também.

## A fila do rádio

Sob interferência o RFCOMM para de escoar; ao voltar, despejaria segundos de quadros velhos na
frente dos novos. O endpoint agora descarta, **antes de cifrar**, o quadro que esperou mais de
250 ms (`VELHO_DEMAIS`). No Windows a ponte aceitava escrita sem limite — o acúmulo ficava depois da
cifragem, onde nada pode ser descartado —, e passou a ter no máximo 4 quadros em voo, com o despertar
testado sem rádio.

## Versão 2 do protocolo

`CURRENT` e `MIN_SUPPORTED` subiram juntos para 2. Uma ponta da versão 1 sobre RFCOMM não confirma
nada; a janela da versão 2 encheria e derrubaria a sessão a cada segundo, em silêncio. Recusar na
negociação diz o motivo. Os vetores gravados foram atualizados **no lugar**, pela exceção de
pré-lançamento de `tests/vectors/main.rs`: o `hello` mudou um byte (a versão), e entrou o vetor de
`reach`.

## As fronteiras que o limite apontou

- **`ir-confiabilidade`**, crate novo e puro: a confiabilidade dos canais e o tempo injetado saíram
  do `ir-session`, que a rota dupla levou a 2 768 linhas de produção. A fronteira já estava provada
  — o módulo só conhecia o `ir-proto` e o tempo, e tinha testes próprios, que vieram junto. O
  `ir-session` o reexporta como `reliability` e `time`, e nada fora dele mudou de nome. Havia
  diretórios vazios com esse nome no disco, fora do git, de uma extração planejada antes.
- **A subida dos portadores e o `Alcance`** saíram do `ir-daemon` para o `ir-transporte`, que é "o
  único lugar que conhece rede e rádio ao mesmo tempo".
- **`ir-configuracao`**: a configuração e a identidade persistentes da máquina saíram do
  `ir-daemon`, para caber a correção do pareamento. Só conheciam a identidade, o papel e a borda; o
  serviço as reexporta como `config`, e nenhum caminho mudou. A próxima coisa que crescer o serviço
  vai precisar de outra fronteira — o servidor de IPC (`ipc/`) é a candidata mais clara.

## A prova

| O quê | Windows | Fedora 44 (container) |
|---|---:|---:|
| Workspace inteiro | 941 testes, 0 falhas | — |
| Crates tocados (`ir-bt`, `-transporte`, `-daemon`, `-session`, `-confiabilidade`, `-configuracao`, `-proto`, `-crypto`, `-net`) | passam | 565 testes, 0 falhas (o `ir-bt` tem 72 lá, com os do BlueZ) |
| `clippy --all-targets` | sem aviso novo | sem aviso novo |
| `xtask` | 251 arquivos dentro das regras | — |

Os testes que carregam a decisão estão em `ir-session/tests/route.rs`: cada quadro sai pelos dois;
a tecla viaja duas vezes e é digitada uma; o cursor anda uma vez (rota dupla e UDP duplicado);
perder o Bluetooth no meio de uma tecla segura mantém a sessão e a tecla; um Bluetooth **calado**,
sem aviso de queda, não custa a sessão por 3 s e o silêncio dele fica visível; os dois calados
derrubam soltando tudo; o portador que volta entra de novo sem aperto de mão; o aperto de mão em
curso absorve o segundo portador; fixar estreita a rota no lugar e soltar a alarga; e o `Reach` chega.
No ator (`ir-daemon/src/actor/alcance/testes.rs`): o rádio contado pelo par é discado e gravado em
disco; a rodada disca só o portador que falta; a discagem sem resposta não se repete antes do prazo,
e a que falha libera a próxima; fixado o Bluetooth, a rede não é discada; a queda de um portador não
derruba o outro; "Encerrar" derruba os dois; **um computador de outra chave pelo rádio não entra
na rota**; e, com um código na tela, um enlace pelo outro portador ou com outra chave não conclui o
pareamento, e a queda dele não o desfaz.

## O que o hardware mostrou

Com autorização do usuário, a versão 2 foi instalada nas duas máquinas da bancada: o RPM no Fedora
por SSH, e o MSI no Windows com elevação pelo UAC. Os registros dos dois serviços, em UTC:

| Hora | O quê |
|---|---|
| 22:43:21–27 | Só o Fedora na v2: a cada tentativa, `erro de protocolo code=UnsupportedMessage fatal=true` — a v1 recusada **com motivo**, e não o enlace mudo que a janela cheia daria |
| 22:43:31 | Windows na v2: `rádio Bluetooth aberto … endereco=74:13:EA:A6:5A:99`; disca o rádio gravado; sessão pelo Bluetooth |
| 22:43:33 | A descoberta acha o par na rede (`10.0.0.135:52525`), o serviço disca a rede, e `rota da sessão mudou route=bluetooth+udp why=Redundant` — **sem aperto de mão novo** |
| 22:43:33 | No Fedora, o UDP recomeçou porque o Windows discou por cima: a rota foi a `bluetooth` e voltou a `bluetooth+udp` no mesmo milissegundo, **sem a sessão cair** |
| 22:44:57 | `bluetoothctl power off` no Fedora: `enlace caiu portador=bluetooth` e `route=udp why=FellBackToNetwork` no mesmo milissegundo, **sem sessão encerrada** |
| 22:44:58–45:16 | O Windows redisca o rádio a cada ~6 s |
| 22:45:18 | Rádio religado às 22:45:17: `route=bluetooth+udp` um segundo depois, sozinho |
| 22:45:31 | Placar do Windows: `rede ouvido há 13498 ms` — **13 s sem rede**, cobertos pelo Bluetooth. É o silêncio do log 24, que antes derrubava a sessão |
| 22:46:02 | **Uma queda por tempo**: os dois portadores calados por mais de 1 s ao mesmo tempo, logo depois dos 13 s sem rede (o SSH para o Fedora também expirou nessa janela). Sessão de volta em 1 s |
| 22:46–22:52 | Nenhuma outra queda. Placar do Fedora às 22:52: **Bluetooth 2 673, rede 2 600** |
| 23:19:08–31 | **Tecla segura à mão durante a queda do rádio.** O usuário manteve uma letra pressionada no teclado do Windows, com o cursor num editor do Fedora; o rádio do Fedora foi desligado por 20 s e religado. A rota foi a `udp` e voltou a `bluetooth+udp` às 23:19:31, sem sessão encerrada nem controle devolvido nos dois registros, e **a repetição da letra no editor não parou em momento nenhum** |

O placar dividido quase ao meio é o dado mais importante: com Wi-Fi de 5 GHz e Bluetooth na mesma
casa, **nenhum dos dois é sempre o mais rápido** — cada quadro chega pelo que estiver melhor
naquele instante, e é essa a latência que a rota dupla entrega.

A queda das 22:46:02 fica registrada como o que é: a rota dupla não cobre os dois calados juntos, e
o prazo de 1 s continua valendo para a rota inteira. A causa de o rádio ter calado junto com a rede
naquele segundo não foi investigada — a coincidência com o SSH expirado aponta para o notebook
inteiro, e não para o rádio.

## O que não foi provado, e por quê

**O Linux não reduz o buffer do socket RFCOMM**, porque o `bluer` não expõe `SO_SNDBUF` e o `unsafe`
do `ir-bt` é só do Winsock. O descarte por idade age antes do kernel.

**O *sniff* continua sem o teste que decide** — `btmon` durante a bancada, procurando `Mode Change`
([log 28](28-a-carga-que-refutou-o-sniff.md)). A rota dupla não depende dele, mas o Bluetooth como
reserva fica melhor sem ele.
