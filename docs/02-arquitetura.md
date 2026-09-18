# 02 — Arquitetura

## 1. Três processos por máquina

```text
 ┌───────────────────────┐        IPC autenticado        ┌────────────────────────┐
 │  inputremote-ui       │◄─────────────────────────────►│                        │
 │  Slint, sem privilégio│   pipe/socket local           │                        │
 │  aberta sob demanda   │                               │                        │
 └───────────────────────┘                               │  inputremote-daemon    │
                                                         │                        │
 ┌───────────────────────┐        IPC autenticado        │  privilegiado          │
 │  inputremote-agent    │◄─────────────────────────────►│  sobe com a máquina    │
 │  dentro da sessão     │   pipe/socket local           │                        │
 │  gráfica / do desktop │                               │  DONO DE TODO O ESTADO │
 └───────────┬───────────┘                               └───────────┬────────────┘
             │                                                       │
   captura e injeção                                 Bluetooth RFCOMM │ UDP │ TCP
   dentro da sessão                                                   │
                                                         ┌────────────▼───────────┐
                                                         │      máquina par       │
                                                         └────────────────────────┘
```

### 1.1. `inputremote-daemon` — o dono de tudo

Serviço do Windows como `LocalSystem`; unidade `systemd` de sistema no Linux, sob o
usuário dedicado `inputremote`.
Sobe no boot, antes de qualquer login, e sobrevive a logoff, troca de usuário e
bloqueio de tela.

Ele é responsável por:

- os três portadores: RFCOMM, UDP de entrada, TCP de dados;
- pareamento, identidades, criptografia Noise;
- **a máquina de estados da sessão e todo o estado dela** — inclusive quais teclas e
  botões estão pressionados neste instante;
- a configuração persistente da máquina;
- o ciclo de vida do agente: subir, vigiar, derrubar, ressubir;
- a injeção de entrada quando não há sessão gráfica de usuário (greeter no Linux);
- os logs e o diagnóstico.

O ponto crítico: **o estado de teclas pressionadas pertence ao serviço, não ao agente.**
O agente é descartável — morre com o logoff, com a troca rápida de usuário, com uma falha
de driver gráfico. Se o estado morasse nele, cada morte deixaria teclas presas, que é a
falha clássica desta classe de software.

### 1.2. `inputremote-agent` — braço dentro da sessão gráfica

Processo sem estado próprio, lançado pelo serviço. Existe **um por sessão de console**.

- **Windows:** lançado como `SYSTEM` com `TokenUIAccess`, em `WinSta0\Default`. Dentro
  dele, **uma thread por desktop** (`Default`, `Winlogon`, `Screen-saver`), cada uma
  amarrada por `SetThreadDesktop` na sua primeira instrução. Só a thread do `Default`
  captura; as outras apenas injetam. Uma quarta thread vigia qual desktop está recebendo
  entrada. Ver [05](05-windows.md) e [ADR-0008](adr/0008-agente-com-thread-por-desktop.md).
- **Linux:** não injeta nunca. O serviço injeta direto por `uinput`, em qualquer situação.
  O agente cuida da captura pelo portal `InputCapture` (papel de servidor), do clipboard e
  do arranjo dos monitores. Ver [06](06-linux.md).

Regra: o agente **NÃO DEVE** tomar decisão nenhuma. Ele recebe comandos já resolvidos
("injete este evento", "suprima a entrada local") e devolve fatos ("este evento
ocorreu", "o desktop mudou"). Toda política vive no serviço.

### 1.3. `inputremote-ui` — configuração

Janela Slint, sem privilégio, aberta pelo usuário. Fala com o serviço pelo IPC. Não lê
nem escreve arquivo de configuração diretamente — pede ao serviço, que é a única fonte
da verdade. Isso elimina as corridas de escrita concorrente que o v1 tinha entre a
janela e o núcleo.

Se a interface travar, for morta ou nunca for aberta, a sessão continua idêntica.

## 2. Fronteiras entre crates

```text
crates/
├── ir-proto/      mensagens, codec, versionamento .............. PURO, sem E/S
├── ir-session/    máquina de estados do produto ................ PURO, sem E/S
├── ir-geometry/   telas, bordas, mapeamento de coordenadas ..... PURO, sem E/S
├── ir-crypto/     Noise, código visual, identidades ........... sem E/S de rede
├── ir-ipc/        protocolo e transporte daemon↔agente↔ui
├── ir-net/        UDP de entrada, TCP de dados, descoberta mDNS
├── ir-bt/         RFCOMM: trait + backend Windows + backend BlueZ
├── ir-transporte/ a fronteira dos portadores: rede e rádio por uma porta só
├── ir-input/      traits de captura/injeção + backends por SO
├── ir-clip/       modelos de clipboard + backends por SO
├── ir-files/      manifesto, blocos, BLAKE3, cotas, staging ......... sem rede
├── ir-transferencia/ a transferência conduzida: o motor ligado à porta
├── ir-daemon/     binário do serviço
├── ir-agent/      binário do agente
└── ir-ui/         interface (Slint): biblioteca testável + binário fino
```

A regra de dependência é uma seta só, e o CI a verifica:

```text
ir-daemon ──► ir-session ──► ir-proto ──► (nada)
    │              └──────► ir-geometry ──► ir-proto
    ├──► ir-transporte ──► ir-net ──► ir-crypto ──► ir-proto
    │                 └──► ir-bt  ──► ir-crypto
    ├──► ir-transferencia ──► ir-files ──► ir-proto
    │                    └──► ir-transporte
    ├──► ir-input
    └──► ir-ipc

ir-agent  ──► ir-ipc, ir-input, ir-clip
ir-ui     ──► ir-ipc          (e mais nada — a interface não conhece o produto)
```

Proibições verificadas automaticamente:

- `ir-proto`, `ir-session` e `ir-geometry` **NÃO DEVEM** depender de `tokio`, de sockets,
  de relógio de parede, de sistema de arquivos ou de qualquer API de sistema operacional;
- `ir-ui` **NÃO DEVE** depender de `ir-session`, `ir-net`, `ir-bt` ou `ir-input`;
- nenhum crate de plataforma (`ir-input`, `ir-bt`, `ir-clip`) depende de outro;
- o `ir-daemon` **NÃO DEVE** falar com `ir-net` ou `ir-bt` direto: quem escolhe o portador é
  o `ir-session`, e quem o alcança é o `ir-transporte`. Foi a ausência dessa fronteira que
  deixou o serviço mandando por um portador o que a sessão marcara para outro.

O `ir-transferencia` nasceu de uma fronteira **comprovada**, e não prevista: o serviço passou de
2 500 linhas de produção no dia em que a transferência entrou nele. É para isso que aquele limite
existe — ele não pede um número maior, pede a fronteira que estava faltando
([09, §1](09-padroes-de-codigo.md), [log 32](logs/32-arquivos-atravessando.md)).

Foi a ausência dessas setas que permitiu ao v1 acumular 10.491 linhas no crate da GUI.

### 2.1. Os dois vocabulários de `ir-ipc`

A seta `ir-ui ──► ir-ipc` só protege alguma coisa se `ir-ipc` **não devolver tipos de
`ir-proto`** para a interface. Se os tipos que a tela desenha forem os tipos do fio, mudar o
formato de fio quebra a interface, e a interface volta a ter opinião sobre protocolo — a seta
estaria cumprida na letra e violada no efeito.

Por isso `ir-ipc` tem dois vocabulários:

| Módulo | Vocabulário | Quem consome |
|---|---|---|
| `status`, `ui`, `vocabulario` | tipos próprios (`Portador`, `Borda`, `Nivel`, `Maquina`, `Nome`, `Recursos`) | `ir-ui` |
| `agent` | tipos de `ir-proto` (`HidUsage`, `Button`, `PointerPosition`) | `ir-agent` |

A exceção do canal do agente é deliberada: ele carrega injeção de entrada, e ali os tipos do
protocolo são exatamente os certos. A interface nunca vê esse módulo, porque `Injetar` não
existe no vocabulário dela ([04, §5](04-seguranca.md)).

As conversões nos dois sentidos vivem em `ir_ipc::vocabulario`, e é o serviço quem traduz. O
efeito colateral é que o nome que aparece na tela deixa de ser o nome técnico: `Portador::nome()`
devolve "Bluetooth", `Portador::nome_tecnico()` devolve "RFCOMM", e só o segundo entra no
diagnóstico.

## 3. O núcleo sem E/S

`ir-session` é uma função sobre estado. Entra um evento, sai uma lista de comandos.

```rust
pub struct Session { /* ... */ }

pub enum Input {
    Tick(Instant),               // relógio injetado, nunca lido
    LinkUp { carrier: Carrier },
    LinkDown { carrier: Carrier, reason: LinkDownReason },
    FrameReceived(Frame),
    LocalPointer(PointerSample),
    LocalKey(KeyEvent),
    EmergencyRelease,
    AgentReady { desktop: DesktopId },
    AgentLost { desktop: DesktopId },
}

pub enum Command {
    SendFrame { carrier: Carrier, frame: Frame },
    Inject(InputEvent),
    ReleaseAll,
    SuppressLocalInput(bool),
    WarpPointer { x: i32, y: i32 },
    StartTimer { id: TimerId, after: Duration },
    Notify(UiEvent),
}

impl Session {
    pub fn step(&mut self, input: Input) -> CommandBatch { /* ... */ }
}
```

Consequências práticas:

- a travessia de borda, a reconexão, a liberação de teclas e a troca de portador são
  testadas com `cargo test`, sem rádio, sem rede e sem segundo computador;
- um bug relatado vira um teste em minutos, porque o estado é reproduzível;
- o relógio é injetado, então cenários de *timeout* rodam instantaneamente;
- não existe `Arc<Mutex<Estado>>` — não há estado compartilhado para haver corrida.

Ver [ADR-0004](adr/0004-nucleo-sans-io.md).

## 4. Concorrência no serviço: um ator central

O serviço tem **uma** tarefa dona do `Session`. Todo o resto são tarefas de E/S que só
convertem bytes em `Input` e `Command` em bytes.

```text
     RFCOMM ──┐                                    ┌──► RFCOMM
        UDP ──┤                                    ├──► UDP
        TCP ──┼─► mpsc<Input> ─► [ tarefa Sessão ] ─┼──► TCP
        IPC ──┤                  (dona do estado)  ├──► IPC (agente)
      timers ─┘                                    └──► IPC (interface)
```

Regras:

- o `Session` **NÃO DEVE** ser acessado de fora dessa tarefa;
- a tarefa da sessão **NÃO DEVE** executar E/S bloqueante nem `await` de rede — ela só
  consome do canal, chama `step()` e despacha comandos;
- toda fila é **limitada**. A fila do ponteiro tem política *o mais recente vence*
  (descarta o antigo); as filas de teclado, botões e controle são confiáveis e, se
  encherem, derrubam o enlace em vez de descartar. Perder um `KeyUp` é pior que cair.

## 5. Caminho de um evento, do início ao fim

Servidor Windows, cliente Windows bloqueado, portador Bluetooth:

```text
 1. teclado físico  →  WH_KEYBOARD_LL no agente do servidor (desktop Default)
 2. o gancho SÓ empilha em fila lock-free e retorna 1 (suprime local)   ◄── ver §6
 3. agente → serviço, por pipe, em lote de no máximo 1 ms
 4. serviço: Session::step(LocalKey) → Command::SendFrame{RFCOMM, ...}
 5. Noise cifra; o quadro vai pelo socket RFCOMM
 ── rádio ──
 6. serviço do cliente decifra, valida sequência, Session::step(FrameReceived)
 7. Session → Command::Inject(KeyDown{scancode})
 8. o agente já sabe, pela thread de vigilância, que o desktop de entrada é "Winlogon"
 9. o comando vai para a thread desk-winlogon, que JÁ EXISTE e já está amarrada a ele
10. essa thread chama SendInput → a senha aparece no campo
```

Os passos 8 e 9 são a resposta ao bug conhecido do Deskflow, em que a tela de bloqueio no
cliente faz o controle voltar para o servidor. A thread do desktop seguro existe desde a
subida do agente justamente para que a troca não custe nada no instante em que a tela
bloqueia — ver [ADR-0008](adr/0008-agente-com-thread-por-desktop.md).

## 6. Regras invioláveis no caminho de latência

Cada uma corresponde a uma falha real e conhecida desta classe de software:

1. **Um gancho de baixo nível do Windows não faz trabalho.** `WH_KEYBOARD_LL` e
   `WH_MOUSE_LL` têm prazo (`LowLevelHooksTimeout`, 300 ms por padrão). Se o
   procedimento demorar, o Windows remove o gancho **em silêncio** — o teclado volta a
   funcionar local e ninguém entende por quê. O procedimento do gancho só empilha em
   fila sem alocação e retorna.
2. **Nenhuma alocação no caminho do evento.** Buffers pré-alocados e reaproveitados.
3. **Nenhum log síncrono no caminho do evento.** `tracing` com escritor sem bloqueio, e
   nível de evento de entrada desligado por padrão.
4. **Nenhum `mutex` disputado.** O caminho agente→serviço→rede é de canais, não de locks.
5. **Movimento do ponteiro é coalescido; teclado nunca.** Se três amostras de movimento
   se acumularem, envia-se a última. Se três teclas se acumularem, enviam-se as três.
6. **Snapshot periódico de estado.** A cada 250 ms o servidor manda o conjunto completo
   de teclas e botões pressionados. É idempotente, e é a rede de segurança contra tecla
   presa, seja qual for a causa da perda.

## 7. Estado persistente

Tudo pertence à máquina, não ao usuário — o serviço precisa dele antes de haver usuário.

| | Windows | Linux |
|---|---|---|
| Configuração | `%ProgramData%\InputRemote\config.toml` | `/etc/inputremote/config.toml` |
| Identidade e pares | `%ProgramData%\InputRemote\state\` | `/var/lib/inputremote/` |
| Logs | `%ProgramData%\InputRemote\logs\` | `journald` |

O material secreto fica em arquivo próprio, com ACL restrita a `SYSTEM` + `Administrators`
no Windows e `0600 root:root` no Linux. Escrita sempre atômica (arquivo temporário +
troca), porque um serviço pode ser morto no meio de uma escrita.

## 8. Modos degradados previstos

Um modo degradado só existe se estiver nesta tabela. Qualquer outro comportamento é bug.

| Situação | Comportamento |
|---|---|
| Bluetooth ausente ou não pareado | entrada por UDP; a interface diz o motivo |
| Rede ausente, Bluetooth ativo | entrada normal; arquivos e imagens marcados como indisponíveis |
| Agente morre no Windows | serviço ressobe em até 500 ms e reenvia o snapshot; teclas não ficam presas |
| Desktop troca (bloqueio, UAC) | a thread daquele desktop já existe; troca instantânea, com snapshot de confirmação |
| Tela de login inalcançável no Windows (nível N2) | tela de bloqueio e UAC funcionam; a tela de login pós-boot exige o teclado físico uma vez, e a interface diz isso |
| Sem sessão gráfica no Linux (greeter) | nada muda: a injeção já é sempre por `uinput` |
| Agente Linux morto | máquina segue como cliente, inclusive bloqueada; perde só o papel de servidor e o clipboard |
| Par sumiu | libera todas as teclas em ≤ 1 s, devolve o controle, tenta reconectar |
| Interface fechada ou morta | nada muda |
| Serviço parado | o agente detecta e se encerra, liberando todas as teclas |
