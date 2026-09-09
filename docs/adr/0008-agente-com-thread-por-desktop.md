# ADR-0008 — Um agente por sessão, com uma thread por desktop

**Status:** aceito · **Data:** 2026-09-09 · **Substitui:** parte do [ADR-0001](0001-tres-processos.md)

O ADR-0001 continua valendo no essencial — três processos, estado no serviço. O que muda
aqui é **quantos agentes existem e como eles alcançam o desktop seguro**.

## Contexto

O desenho original lançava **um processo de agente por desktop**: um para `Default`, e
outro criado no instante em que o desktop de entrada mudasse para `Winlogon`.

A leitura do Deskflow mostrou o problema ([00b, §1](../00-licoes-do-deskflow.md)). O
processo do `Winlogon` só nasceria quando a tela bloqueasse. O usuário pagaria, exatamente
no momento em que quer digitar a senha:

1. criação de processo (dezenas a centenas de milissegundos);
2. abertura do desktop e instalação de ganchos;
3. espera do `AgentReady` pelo serviço, com prazo de 2 s;
4. reenvio do `StateSnapshot`.

Ou seja: a pior latência do produto cairia no momento mais visível dele. E, se qualquer um
dos passos falhasse, o sintoma seria "o teclado não funciona na tela de bloqueio" — o
sintoma que o produto inteiro existe para eliminar.

## Decisão

**Um agente por sessão de console**, lançado pelo serviço em `WinSta0\Default`, como
`SYSTEM` com `TokenUIAccess`. Dentro dele, **uma thread por desktop**:

| Thread | Desktop | Faz |
|---|---|---|
| `desk-default` | `Default` | injeta **e** captura |
| `desk-winlogon` | `Winlogon` | **só injeta** — nunca instala gancho de captura |
| `desk-screensaver` | `Screen-saver` | só injeta |
| `desk-watch` | nenhum | vigia qual é o desktop de entrada, a cada 200 ms |

Cada thread de desktop chama `OpenDesktop` + `SetThreadDesktop` como **primeira ação**,
antes de criar qualquer janela ou gancho — depois disso, `SetThreadDesktop` falha.

As threads são criadas na subida do agente, não sob demanda. Quando o desktop de entrada
muda, a única coisa que acontece é qual thread recebe o comando de injeção. Não há criação
de processo, não há instalação de gancho, não há espera.

O serviço continua sendo o dono do estado e continua recebendo `DesktopChanged` e
reenviando `StateSnapshot` — agora como confirmação barata, não como reconstrução.

## A thread do `Winlogon` nunca captura

Regra de segurança, não de desempenho: instalar um gancho de teclado no desktop seguro
significa ler o que é digitado na tela de bloqueio **da própria máquina**. É o
comportamento literal de um keylogger, e não há necessidade dele — quando o servidor
bloqueia, o controle volta para local de qualquer forma.

Portanto: injeção nos três desktops, captura só no `Default`. Ver [04](../04-seguranca.md).

## Alternativas descartadas

**Um processo por desktop.** O desenho anterior. Isolamento melhor, latência inaceitável
no momento errado.

**Um processo só, no `Default`, injetando sem trocar de desktop.** Não funciona:
`SendInput` entrega ao desktop da thread que chama, e a thread do `Default` acerta um
desktop que ninguém está vendo. É a causa do bug conhecido do Deskflow
([#7899](https://github.com/deskflow/deskflow/issues/7899)).

**Mover a thread existente com `SetThreadDesktop` quando o desktop muda.** Falha: a
chamada é recusada depois que a thread cria janela ou gancho. Por isso as threads são
fixas, uma por desktop, criadas antes de qualquer coisa.

## Consequências

**Boas.**
- Troca de desktop instantânea: a thread do `Winlogon` já existe e já está pronta quando a
  tela bloqueia.
- Um processo de agente por sessão, em vez de N — menos IPC, menos ciclo de vida.
- O lançamento fica mais simples: `lpDesktop` é sempre `WinSta0\Default`.
- Continua valendo que o estado é do serviço: se o agente inteiro morrer, ele ressobe e
  recebe o `StateSnapshot`.

**Ruins, e aceitas.**
- Uma falha derruba todos os desktops de uma vez, em vez de um. Mitigado pelo estado morar
  no serviço e pelo reinício em menos de 500 ms.
- Threads amarradas a desktops são delicadas: `SetThreadDesktop` precisa ser a primeira
  chamada, e isso é fácil de quebrar sem perceber. Vira teste e comentário obrigatório.
- Há threads vivas no desktop seguro o tempo todo. É justamente o que dá a latência zero,
  mas exige a regra de "nunca capturar ali" para ser defensável.
- Cada thread precisa do próprio laço de mensagens, e nenhuma pode bloquear.
