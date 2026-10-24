# O painel no Android: o uso dos dois computadores numa tela à parte

**Data:** 2026-10-24 · **Branch:** `feat/painel-android`

**Itens:** [12](../12-painel-android.md), [ADR-0018](../adr/0018-painel-no-android.md);
[PROGRESSO](../../PROGRESSO.md), Etapa 12.

**O que foi pedido:** "usar um tablet ou celular Android para monitorar o uso de CPU, RAM e
temperaturas dos sensores e latência do Bluetooth e rede em formato de dashboard, uma APK simples
que conecta do sistema e pareado […] deve ser bem leve o uso de recursos dos computadores." Depois
de uma entrevista que virou o documento 12: "implementar toda a ideia de forma organizada, buscar a
ótima usabilidade, experiência de usuário, princípios SOLID, DRY e boas práticas."

## O que foi feito, em ordem de commit

1. **Protocolo 11 e latência por rota** (`ir-proto`, `ir-session`). `UsageSample` em ponto fixo,
   `Control::WatchUsage` e `Control::Usage` com o portão `supports_usage`, quatro vetores gravados.
   O `Pong` passou a ser medido por portador **antes** da detecção de repetição: na rota dupla a
   cópia do portador mais lento era descartada sem ser medida, e não havia como saber o Bluetooth e
   a rede separados.
2. **`ir-telemetria`.** Uma `Fonte` por métrica, cada uma na sua cadência; a `Vigia` é uma linha que
   só existe enquanto alguém olha. O texto de `/proc` e `/sys` e os identificadores do
   LibreHardwareMonitor viram número num módulo puro, testado em qualquer sistema.
3. **`ir-monitor`**, puro: a conversa cifrada que as duas pontas usam, o vocabulário do painel com
   vetores, a mesa e os níveis.
4. **`ir-monitor-rede`**: a central dona do estado, uma tarefa por painel, a porta que só abre com
   alguém a atender, a assinatura que expira.
5. **O serviço** (`ir-vigilancia`, mais a configuração e o `ir-ipc`).
6. **`ir-ui-servico`** e **a tela do PC**.
7. **`ir-monitor-android`** e **o aplicativo**.

## O teto de tamanho

O serviço estava em **2 498** linhas de produção e a janela em **2 493**, contra o teto de 2 500. O
limite não subiu. O painel entrou em crates próprios, e três coisas que já tinham fronteira foram
para ela: subir os canais locais (`ir-canais::abrir`), o registro dos avisos da sessão
(`ir-painel::registrar`) e o serviço simulado da janela (`ir-ui-servico`). A ligação no serviço
ficou em dirigir a sessão com as entradas que a vigilância devolve. **O serviço fechou com 2 490
linhas, oito a menos que antes.**

## O que foi provado, e como

- **Rust:** os testes de todos os crates mexidos, `clippy -D warnings` e `cargo xtask check` no
  Windows; `ir-telemetria` também no contêiner Fedora 44.
- **As fontes**, com dados reais: `cargo run -p ir-telemetria --example amostra` nos dois sistemas.
  No Windows, como usuário comum, a zona térmica do ACPI volta vazia — o serviço a lê como `SYSTEM`,
  e isso fica para a bancada.
- **O monitor e o cliente, de verdade, em TCP no loopback**: pareia conferindo o mesmo código,
  recebe o quadro, volta sem código pela chave gravada (`ir-monitor-android/tests/ponta_a_ponta.rs`).
- **A janela do PC** em `--simulado`, conduzida pela automação de acessibilidade do Windows (os
  botões do Slint se anunciam pelo texto): cartão, tela, código, confirmação e lista.
- **O aplicativo no emulador** (Tablet API 36) contra a bancada (`--example bancada`): a descoberta
  achou o PC pela UDP 52524; o código **642 678** apareceu nas duas pontas; o painel em pé e
  deitado, com os níveis de verdade (a CPU a 99 % pôs a borda em vermelho); girar não derrubou a
  ligação; depois de reinstalar, o aplicativo voltou sem código.
- **11 testes de JVM** no aplicativo: leitura do JSON, histórico, e o `Controlador` com uma fonte de
  mentira (procurar, ligar, parear, recusado, religar sozinho).

## O que deu errado no caminho

- **A recusa do código não chegava ao outro lado.** `Conversa::confirmar(false)` devolvia erro e
  descartava os bytes da recusa; o outro lado ficaria esperando o prazo. Agora a recusa sai, e a
  conversa fica encerrada depois dela. Pego pelo teste antes do primeiro commit.
- **Porcentagem truncada**: 349,99 milésimos apareciam como "34 %". Arredonda.
- **No tablet deitado, a bateria e a temperatura ficavam cortadas** embaixo do cartão. O conteúdo
  do cartão rola, quando eles ficam lado a lado. Pego pela captura do emulador.
- **O `Controlador`, antes de compilar**: lia o cofre a cada quadro, tratava a queda depois de um
  pareamento bem-sucedido como pareamento falho, e religaria pareando de novo. Corrigido na leitura,
  antes do primeiro build.
- **A bancada desta máquina**: o AVD tem outro aplicativo que se fixa na tela e o "Fixar app" do
  Android apareceu por cima, uma vez; não é deste aplicativo, e nada nele foi mexido.

## Decisões que mudaram em relação ao documento 12

- **JNI em vez de UniFFI**, **descoberta em vez de QR**, e a latência por rota medida no `Pong` que
  já existia. Os motivos estão no [ADR-0018](../adr/0018-painel-no-android.md).

## O que falta

- A bancada com o serviço **instalado** nos dois computadores e um aparelho de verdade — inclusive
  o par antigo pedindo atualização e as duas rotas medidas na rota dupla.
- Medir o orçamento de [12, §3.4](../12-painel-android.md).
- A temperatura do Windows como `SYSTEM`.
- Uma noite no protetor de tela, no carregador.
- Onde publicar o APK, e com que chave.
