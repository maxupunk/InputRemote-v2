# 00b — Lições do Deskflow

O Deskflow é o sucessor aberto do Synergy e o software mais próximo deste projeto: mesmo
problema, vinte anos de campo, código legível. Foi lido para decidir, não para copiar.

Cada item traz o que foi observado no código ou nas issues, e o que decidimos.

## 1. Thread por desktop, não processo por desktop — **adotado**

`MSWindowsDesks.cpp` mantém **uma thread por desktop** dentro de um único processo. Cada
thread chama `SetThreadDesktop` como primeira ação, roda seu próprio laço de mensagens e
instala seus próprios ganchos.

Isso invalidou o nosso desenho anterior, que lançava **um processo por desktop**. O
problema do nosso: o processo do desktop `Winlogon` só nasceria no instante em que a tela
bloqueasse, e o usuário pagaria a latência de criar processo, instalar ganchos e
ressincronizar estado exatamente no momento em que quer digitar a senha.

Com thread por desktop, a thread do `Winlogon` já existe e já está pronta antes de a tela
bloquear. A troca passa a ser "qual thread chama `SendInput`" — instantânea.

Ver [ADR-0008](adr/0008-agente-com-thread-por-desktop.md).

## 2. A notificação de troca de desktop não existe — **confirmado**

Há um comentário no `MSWindowsDesks.cpp` dizendo, em essência, que seria bom se o Windows
notificasse a troca de desktop, mas que até onde o autor sabe ele não notifica. A solução
deles é um temporizador de **0,2 s** chamando `handleCheckDesk()`.

Chegamos ao mesmo intervalo por raciocínio independente. A confirmação vale porque elimina
a tentação de "achar a API certa depois": ela não existe. `WTS_SESSION_LOCK` ajuda, mas o
desktop seguro do UAC não gera notificação nenhuma — a vigilância por consulta é
obrigatória, não é preguiça.

## 3. Sincronizar teclas ao trocar de desktop — **confirmado**

No `deskThread()`, a troca de desk dispara `updateKeys()`, com o comentário de que é
preciso sempre sincronizar as teclas ao trocar de desk para os modificadores ficarem
corretos.

É exatamente o nosso `StateSnapshot` ([03, §7](03-protocolo.md)), a que chegamos pelo
requisito de "zero teclas presas". Duas implementações convergindo no mesmo mecanismo é o
sinal mais forte de que ele é necessário.

## 4. O processo é lançado no `Default`, não no `Winlogon` — **adotado**

`MSWindowsProcess.cpp` fixa `si.lpDesktop` em `winsta0\Default`. O acesso ao desktop
seguro vem depois, de dentro do processo, por `OpenDesktop` + `SetThreadDesktop`.

Nosso desenho anterior passava `WinSta0\Winlogon` no `STARTUPINFO`. Desnecessário: com
threads por desktop, um lançamento no `Default` basta e é mais simples.

## 5. `TokenUIAccess` já era feito lá — **adotado, e agora obrigatório**

O vigia do Windows duplica um token e marca `TokenUIAccess` antes de criar o processo.
Em 2026 isso deixou de ser refinamento: com o endurecimento de janeiro
([05, §4.4](05-windows.md)), UIAccess é uma das três origens de entrada aceitas pelas
interfaces de credencial.

## 6. O gancho enfileira em vez de trabalhar — **adotado, com regra mais dura**

`keyboardLLHook()` e `mouseLLHook()` chamam `PostThreadMessage` e retornam. Não fazem
trabalho no procedimento do gancho, pelo motivo que documentamos em
[02, §6](02-arquitetura.md): o Windows remove em silêncio ganchos que estouram o prazo.

Nossa regra é mais dura: **fila circular sem alocação e sem chamada de sistema**.
`PostThreadMessage` é uma chamada ao kernel dentro do caminho mais sensível que existe.

## 7. Caminho de gancho por injeção de DLL — **rejeitado**

`MSWindowsHook.cpp` mantém `kHOOK_OKAY` além de `kHOOK_OKAY_LL`, e um `WH_GETMESSAGE`
para detectar protetor de tela. É herança da era anterior aos ganchos de baixo nível.

Não entra aqui. Dois caminhos de captura significam dois comportamentos, dois conjuntos de
bugs e o dobro de superfície — e injeção de DLL em processos alheios é exatamente o que
antivírus tratam como ataque. Protetor de tela se detecta pelo nome do desktop, que já
estamos consultando de qualquer forma. Ver [11](11-nao-legado.md).

## 8. A tela de bloqueio é um problema em aberto lá — **é o nosso requisito central**

As issues contam a história:

| Issue | Relato |
|---|---|
| [#7899](https://github.com/deskflow/deskflow/issues/7899) | tela de bloqueio ou UAC no cliente faz o ponteiro voltar para o servidor |
| [#8183](https://github.com/deskflow/deskflow/issues/8183) | não consegue controlar diálogo de UAC nem tela de login com auto-elevação |
| [#7964](https://github.com/deskflow/deskflow/issues/7964) | cliente bloqueia por inatividade e não há como desbloquear pelo teclado compartilhado — fechada como "não é nosso bug" |
| [#5294](https://github.com/deskflow/deskflow/issues/5294) | ponteiro travado na tela de login de servidor Windows |

E a FAQ do projeto é explícita: o daemon existe para os prompts de UAC, e funcionar na
tela de login é um efeito colateral feliz disso — não um objetivo perseguido.

**É aqui que os dois projetos divergem.** Para o Deskflow, a tela de bloqueio é um caso de
borda que às vezes funciona. Para nós ela é o **piso** — o nível N2 de
[01, §2](01-visao-e-escopo.md) — e é o motivo de a arquitetura inteira ser como é. O que
lá é "não é nosso bug", aqui é critério de aceitação obrigatório
([10, §6](10-testes-e-validacao.md), passos 6 e 7; o passo 8, a tela de login, é o alvo N3,
não o piso).

## 9. O que não olhamos, e por quê

Protocolo, descoberta e clipboard do Deskflow não foram estudados. O protocolo deles é
texto sobre TCP, de compatibilidade longa com o Synergy, e carrega decisões de 2001. Não
há Bluetooth. Nossas escolhas nessas áreas são independentes e estão em
[03](03-protocolo.md) e [ADR-0003](adr/0003-noise-em-vez-de-quic.md).

## 10. Licença

O Deskflow é GPLv2. **Nenhuma linha de código dele entra neste projeto.** O que foi
aproveitado são fatos sobre o comportamento do Windows — que não são propriedade de
ninguém — e decisões de projeto reimplementadas do zero a partir desses fatos.

## 11. Fontes

Arquivos lidos no repositório do Deskflow, em 09/09/2026, ramo `master`:

| Arquivo | O que respondeu |
|---|---|
| [`MSWindowsDesks.cpp`](https://github.com/deskflow/deskflow/blob/master/src/lib/platform/MSWindowsDesks.cpp) | thread por desktop, `OpenInputDesktop`, temporizador de 0,2 s, `updateKeys()` na troca |
| [`MSWindowsProcess.cpp`](https://github.com/deskflow/deskflow/blob/master/src/lib/platform/MSWindowsProcess.cpp) | `si.lpDesktop` fixo em `winsta0\Default`, sinalizadores de criação |
| [`MSWindowsWatchdog.cpp`](https://github.com/deskflow/deskflow/blob/master/src/lib/platform/MSWindowsWatchdog.cpp) | duplicação de token, `TokenUIAccess`, laço de vigilância |
| [`MSWindowsSession.cpp`](https://github.com/deskflow/deskflow/blob/master/src/lib/platform/MSWindowsSession.cpp) | `WTSGetActiveConsoleSessionId` para detectar troca de sessão |
| [`MSWindowsHook.cpp`](https://github.com/deskflow/deskflow/blob/master/src/lib/platform/MSWindowsHook.cpp) | ganchos de baixo nível, `PostThreadMessage`, caminho legado `kHOOK_OKAY` |

Issues e documentação: [#7899](https://github.com/deskflow/deskflow/issues/7899),
[#8183](https://github.com/deskflow/deskflow/issues/8183),
[#7964](https://github.com/deskflow/deskflow/issues/7964),
[#5294](https://github.com/deskflow/deskflow/issues/5294),
[Legacy FAQ](https://github.com/deskflow/deskflow/wiki/Legacy-FAQ),
[Workarounds](https://github.com/deskflow/deskflow/wiki/Workarounds).
