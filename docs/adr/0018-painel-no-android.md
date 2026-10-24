# ADR-0018 — O painel no Android: um par que só lê

**Status:** aceito · **Data:** 2026-10-24 · **Origem:** [12](../12-painel-android.md), entrevista com o
usuário · ([log 68](../logs/68-o-painel-no-android.md))

## O que se quer

Um tablet ou celular Android, pareado com o sistema, mostrando numa tela fixa o uso dos dois
computadores — processador, memória, disco, rede, bateria, temperaturas — e a latência de cada rota
entre eles. Leve para os computadores, e sem abrir exceção na segurança.

## A decisão

**O painel é um par que só lê.** Mesmo Noise e mesmo código de seis dígitos do pareamento entre os
computadores ([04, §3](../04-seguranca.md)), mas num vocabulário que não tem como pedir nada além de
ver, e numa porta própria.

1. **Um computador agrega.** O painel pareia com um computador só, que lhe entrega o próprio uso e o
   do par. O pedido ao par vai pela sessão que já existe: `Control::WatchUsage` e `Control::Usage`,
   **protocolo 11**, só para um par que negociou a 11.
2. **Porta 52527/TCP, aberta só quando há a quem atender** — um painel pareado, ou a janela de
   pareamento aberta por três minutos. A regra de firewall do Windows é por programa e já a cobre; o
   serviço do firewalld ganhou a porta.
3. **Custo zero sem ninguém olhando.** A coleta (`ir-telemetria`) só existe com um painel assinando
   ou com o par pedindo; as duas assinaturas **expiram** em 5 s sem renovação, então um tablet que
   some da rede sem avisar não deixa a coleta ligada.
4. **A chave do painel fica longe dos pares que digitam**: `paineis` no `config.toml`, e não
   `peers`. Nenhum caminho que dá entrada, clipboard ou arquivo a um par consegue achá-lo.
5. **A latência de cada rota sai do `Pong` que já existe**, medido por portador **antes** da
   detecção de repetição (`Notice::RouteLatency`). Na rota dupla o par responde ao primeiro `Ping`
   que chega, então a medida da rota mais lenta é a ida pela mais rápida somada à volta por ela: uma
   estimativa honesta de quanto aquela rota atrasa, e exata com um portador só.

## Os crates

| Crate | O quê | Depende de |
|---|---|---|
| `ir-telemetria` | a coleta, uma `Fonte` por métrica, na cadência de cada uma | `ir-proto` |
| `ir-monitor` | puro: a conversa cifrada, o vocabulário do painel, a mesa e os níveis | `ir-proto`, `ir-crypto` |
| `ir-monitor-rede` | a porta, uma tarefa por painel, a janela de pareamento | `ir-crypto`, `ir-monitor` |
| `ir-vigilancia` | o painel visto pelo serviço: quando coletar, o que pedir ao par, a lista | os de cima, `ir-session`, `ir-ipc`, `ir-configuracao` |
| `ir-monitor-android` | o núcleo do aplicativo: cliente, descoberta, cofre, JNI | `ir-proto`, `ir-crypto`, `ir-monitor`, `ir-net` |
| `ir-ui-servico` | o contrato `Servico` e o serviço simulado, tirados da `ir-ui` | `ir-ipc` |

O serviço e a janela estavam a duas e sete linhas do teto de 2 500 ([09, §1](../09-padroes-de-codigo.md)).
O limite não subiu: o painel entrou em crates próprios, e o que já tinha fronteira saiu para ela —
subir os canais locais foi para o `ir-canais`, o registro dos avisos da sessão para o `ir-painel`, o
simulado para o `ir-ui-servico`. O serviço fechou menor do que estava.

## Dependências novas

- **`wmi` 0.18** (Windows, `ir-telemetria`): a zona térmica do ACPI e o WMI do
  LibreHardwareMonitor, sem `unsafe` nosso. Usa o mesmo `windows` 0.62 do workspace. Na primeira
  conexão numa linha sem COM ele chama `CoInitializeSecurity` com o padrão e tolera
  `RPC_E_TOO_LATE`; o serviço não usa COM em outro lugar.
- **`jni` 0.22** (`ir-monitor-android`): a mesma versão que o Slint já trazia ao `Cargo.lock`.
- **`serde_json`** (`ir-monitor-android`, e só para teste no `ir-monitor`): o contrato com o Kotlin.

## `unsafe`

Três exceções novas à tabela de [09, §4](../09-padroes-de-codigo.md), cada uma embrulhada em função
segura no próprio módulo:

| Crate | Módulo | Por quê |
|---|---|---|
| `ir-telemetria` | `windows::sistema` | `GetSystemTimes`, `GlobalMemoryStatusEx`, `GetDiskFreeSpaceExW`, `GetIfTable2`, `GetSystemPowerStatus` |
| `ir-telemetria` | `linux::sys` | `statvfs` |
| `ir-monitor-android` | `jni` | só `#[unsafe(no_mangle)]`: o corpo de cada função é Rust seguro |

## Alternativas descartadas

- **Temperatura com o driver do LibreHardwareMonitor embutido.** Cobriria mais sensores, mas é um
  driver de núcleo que o Defender bloqueia como vulnerável, e o produto não instala driver
  ([11](../11-nao-legado.md)). Lê-se o LHM quando ele já está rodando; senão, o ACPI; senão, "—".
- **UniFFI**, que o documento 12 previa. Exigiria gerar os *bindings* a cada build e levar o JNA ao
  aparelho (~1 MB a mais, reflexão a cada chamada). A superfície é de nove funções, e o JNI direto
  com eventos em JSON — lidos pelo `org.json` que já vem no Android — é menor e não tem passo de
  geração.
- **QR code no pareamento**, que o documento 12 previa. Exigiria câmera e uma biblioteca de leitura
  no aplicativo. A descoberta UDP 52524 já acha o computador sem nada a ler, e quem autentica é o
  código de seis dígitos, não o QR. O IP digitado continua como plano B.
- **Bluetooth para o painel.** Disputaria o rádio que leva teclado e mouse.
- **Cada computador falando com o painel.** O dobro de pareamentos, e um segundo caminho para
  manter. O preço de um computador só: com ele desligado, o painel não mostra o outro.
- **Pedido ao par sem expiração.** Um pedido de "pare" perdido — o painel saiu da rede, o
  computador dormiu — deixaria o par coletando para sempre.

## O custo aceito

- O painel depende do computador que agrega estar ligado.
- A latência da rota mais lenta, na rota dupla, é uma estimativa (item 5).
- No Windows sem o LibreHardwareMonitor e sem zona térmica no ACPI, não há temperatura.
- O aplicativo é um segundo artefato para assinar e distribuir.
