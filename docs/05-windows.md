# 05 — Plataforma Windows

Este documento trata do requisito mais difícil do produto: funcionar na tela de bloqueio.
Tudo aqui existe por causa do modelo de *window stations* e *desktops* do Windows.

## 1. O modelo que dita a arquitetura

Uma sessão interativa tem uma window station, `WinSta0`, com três desktops:

| Desktop | Quando está recebendo entrada |
|---|---|
| `Default` | uso normal, usuário logado e desbloqueado |
| `Winlogon` | tela de login, tela de bloqueio, prompts de UAC — o *desktop seguro* |
| `Screen-saver` | protetor de tela |

Quatro fatos, e as consequências de cada um:

1. **Um serviço vive na sessão 0, que não tem desktop interativo.** Ele não pode chamar
   `SendInput` e alcançar o usuário. → daí o agente existir.
2. **As threads de um processo são amarradas a um desktop na criação** e não migram
   depois que criaram janelas ou ganchos. → daí o agente ser reiniciado, não realocado.
3. **O acesso ao desktop `Winlogon` é dado pela DACL dele**, que concede a `SYSTEM` e não
   ao usuário interativo. (`SeTcbPrivilege` é exigido em outro ponto: para mover o token
   de sessão e para marcar UIAccess — §3.1.) → daí o agente rodar como `SYSTEM`.
4. **`SendInput` entrega ao desktop da própria thread.** Um agente no `Default` chamando
   `SendInput` enquanto a tela está bloqueada não erra: ele acerta um desktop que ninguém
   está vendo. → daí ser obrigatório detectar a troca de desktop.

O item 4 é a causa do bug conhecido do Deskflow em que a tela de bloqueio no cliente faz
o controle voltar para o servidor ([#7899](https://github.com/deskflow/deskflow/issues/7899),
[#8183](https://github.com/deskflow/deskflow/issues/8183)). Não é um caso de borda: é o
caminho principal do nosso requisito.

> ⚠ **Há um quinto fato, novo e decisivo:** desde a atualização de segurança de janeiro de
> 2026, as interfaces de credencial do Windows recusam entrada injetada, salvo de três
> origens. Ele determina como o agente precisa ser construído, assinado e instalado —
> **leia a §4.4 antes de qualquer outra coisa neste documento.**

## 2. O serviço

- Tipo `SERVICE_WIN32_OWN_PROCESS`, conta `LocalSystem`, início **automático**
  (não "automático (atraso)" — a tela de login aparece cedo demais para isso).
- Recuperação: reiniciar após falha, em 5 s / 5 s / 60 s.
- Aceita `SERVICE_ACCEPT_STOP`, `SERVICE_ACCEPT_SESSIONCHANGE` e
  `SERVICE_ACCEPT_POWEREVENT`.
- `RequiredPrivileges` mínimo: `SeTcbPrivilege`, `SeAssignPrimaryTokenPrivilege`,
  `SeIncreaseQuotaPrivilege`.

### 2.1. Eventos que o serviço trata

| Evento | Ação |
|---|---|
| `WTS_CONSOLE_CONNECT` | sessão de console mudou → derruba agentes antigos, sobe na nova |
| `WTS_SESSION_LOCK` / `UNLOCK` | caminho rápido de troca de desktop (§3.3) |
| `WTS_SESSION_LOGON` / `LOGOFF` | idem; em `LOGOFF`, `ReleaseAll` antes de qualquer coisa |
| `PBT_APMSUSPEND` | `ReleaseAll`, fecha enlaces, marca sessão como suspensa |
| `PBT_APMRESUMEAUTOMATIC` | reabre enlaces, envia `StateSnapshot` |
| `SERVICE_CONTROL_STOP` | `ReleaseAll`, avisa o par, encerra agentes, sai |

`ReleaseAll` antes de encerrar é obrigatório em todos os caminhos. Um serviço que morre
com `Ctrl` pressionado deixa a máquina inutilizável até o próximo reinício.

## 3. Ciclo de vida do agente

É **um agente por sessão de console**, com uma thread por desktop dentro dele.
Ver [ADR-0008](adr/0008-agente-com-thread-por-desktop.md) para por que não é um processo
por desktop.

### 3.1. Lançar o agente

```text
1. WTSGetActiveConsoleSessionId()            → sessão de console (existe antes do login)
2. OpenProcessToken(GetCurrentProcess())     → token do próprio serviço (SYSTEM)
3. DuplicateTokenEx(..., TokenPrimary)       → token primário duplicado
4. SetTokenInformation(TokenSessionId, id)   → move o token para a sessão de console
                                               (é aqui que SeTcbPrivilege é exigido)
5. SetTokenInformation(TokenUIAccess, 1)     → marca UIAccess; ver §4.4 — também exige
                                               SeTcbPrivilege
6. STARTUPINFO.lpDesktop = "WinSta0\\Default"    → sempre Default; o desktop seguro é
                                                   alcançado de dentro, por thread
7. CreateProcessAsUser(token, ..., CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW)
```

O passo 5 só tem efeito se o executável do agente estiver assinado e instalado em local
gravável apenas por administradores (§4.4).

O agente nasce sabendo duas coisas, por linha de comando e variável de ambiente: o nome do
pipe de agente e um segredo de uma via para se autenticar no serviço
([04, §5](04-seguranca.md)). Ele **NÃO DEVE** aceitar comando de mais ninguém.

### 3.2. As threads do agente

Criadas na subida, todas, antes de qualquer desktop ser necessário:

| Thread | Desktop | Papel |
|---|---|---|
| `desk-default` | `Default` | injeta **e** captura |
| `desk-winlogon` | `Winlogon` | **só injeta** — nunca instala gancho |
| `desk-screensaver` | `Screen-saver` | só injeta |
| `desk-watch` | nenhum | vigia qual desktop está recebendo entrada |

Cada thread de desktop faz, **como primeira ação**:

```text
h = OpenDesktop("Winlogon", 0, FALSE, GENERIC_ALL)
SetThreadDesktop(h)
// só a partir daqui: laço de mensagens, ganchos, SendInput
```

A ordem não é estilo: `SetThreadDesktop` é recusado depois que a thread cria qualquer
janela ou gancho. Errar isso produz uma thread que roda, não dá erro e injeta no desktop
errado — falha silenciosa, a pior categoria.

**A thread do `Winlogon` nunca captura.** Instalar gancho de teclado no desktop seguro é
ler o que se digita na tela de bloqueio da própria máquina — comportamento literal de
keylogger, sem necessidade nenhuma: quando o servidor bloqueia, o controle volta para
local de qualquer forma.

### 3.3. Quem observa a troca de desktop

Um serviço na sessão 0 **não consegue** consultar o desktop de entrada da sessão 1:
`OpenInputDesktop` responde sobre a sessão de quem chama. Quem observa é a thread
`desk-watch`, que já está do lado certo.

```text
loop a cada 200 ms:
    h = OpenInputDesktop(0, FALSE, DESKTOP_READOBJECTS)
    nome = GetUserObjectInformation(h, UOI_NAME)
    se nome != desktop_corrente:
        roteia a injeção para a thread daquele desktop
        avisa o serviço: DesktopChanged{nome}
```

Não existe notificação do sistema para isso — o próprio Deskflow registra em comentário
que tentou e o Windows não notifica ([00b, §2](00-licoes-do-deskflow.md)). A consulta
periódica é obrigatória.

O serviço, ao receber `DesktopChanged`, reenvia o `StateSnapshot`. Como as threads já
existem, isso é confirmação barata, não reconstrução: a troca custa o tempo de trocar um
ponteiro, e a tela de bloqueio já aceita a primeira tecla.

### 3.4. Caminho rápido

`WTS_SESSION_LOCK` e `WTS_SESSION_UNLOCK` chegam ao serviço direto e antecipam o
`StateSnapshot` sem esperar os 200 ms. São otimização, **não** substituem a vigilância: o
desktop seguro do UAC não gera notificação de sessão nenhuma.

### 3.5. Se o agente morrer

O serviço detecta a queda do pipe e ressobe o agente em menos de 500 ms, reenviando o
`StateSnapshot` após o `AgentReady`. Enquanto não houver agente pronto, a entrada é
**enfileirada por no máximo 300 ms**; passado isso, é descartada e um `ReleaseAll` é
emitido. Melhor perder eventos do que injetá-los tarde, fora de contexto, num campo de
senha.

### 3.6. Níveis N3 e N2 no Windows

Os níveis de capacidade estão definidos em [01, §2](01-visao-e-escopo.md). No Windows, a
diferença entre N3 (tela de login) e N2 (tela de bloqueio) **não é o mecanismo de
injeção** — os dois são o mesmo desktop `Winlogon`, com a mesma thread e a mesma chamada.

A diferença é o que precisa estar pronto antes:

| Precisa estar pronto | N2 (bloqueio) | N3 (login) |
|---|---|---|
| Serviço rodando | sim, já está há tempo | **tem de ganhar a corrida com o LogonUI** |
| Sessão de console existente | sim | `WTSGetActiveConsoleSessionId` pode responder `0xFFFFFFFF` nos primeiros instantes |
| Pilha de rede com endereço | sim | DHCP pode levar segundos |
| Rádio Bluetooth inicializado e par conhecido | sim | inicialização do rádio leva tempo; as chaves são da máquina, então o par é conhecido |
| Servidor sabendo o endereço do cliente | sim, sessão anterior | descoberta precisa acontecer do zero |

Ou seja: **N3 é N2 mais uma corrida de inicialização.** As respostas de projeto:

- início do serviço **automático**, nunca "automático (atraso)";
- `WTSGetActiveConsoleSessionId` é consultado em laço com espera curta até responder uma
  sessão válida, em vez de uma vez só na subida;
- o serviço só se declara pronto depois de agente vivo **e** ao menos um portador
  disponível, e registra quanto tempo levou — o número aparece no diagnóstico;
- o servidor mantém o último endereço conhecido do par e tenta por ele antes de descobrir.

Existe uma segunda diferença possível, e ela não está sob nosso controle: a Microsoft pode
tratar a tela de login e a tela de bloqueio de forma diferente no endurecimento da §4.4. A
documentação não separa as duas. Por isso a PoC-1 mede as duas **separadamente**, e o
resultado pode ser N2 no Windows e N3 no Linux — o que é um resultado válido, desde que
declarado ([01, §2](01-visao-e-escopo.md)).

**Se N3 não for alcançável no Windows**, o produto entrega N2 ali: teclado e mouse
funcionam na tela de bloqueio e em prompts de UAC, e a tela de login logo após o boot exige
o teclado físico daquela máquina uma vez. A limitação vai para o README e para a interface.
Isso **não** bloqueia o lançamento.

## 4. Injeção

### 4.1. Teclado

`SendInput` com `KEYEVENTF_SCANCODE`, nunca com código virtual. O HID Usage recebido
([03, §5](03-protocolo.md)) vira scancode PS/2 por tabela estática, com
`KEYEVENTF_EXTENDEDKEY` quando o scancode for estendido (`0xE0`).

Injetar por scancode faz o layout do **computador controlado** decidir o caractere — que
é exatamente o que o requisito da tela de login precisa.

### 4.2. Ponteiro — sempre absoluto

Movimento é injetado com `MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`,
com as coordenadas normalizadas em `0..65535` sobre o desktop virtual.

Não se injeta movimento relativo. Motivo: o Windows aplica aceleração ("balística") ao
movimento relativo injetado, e o servidor já entregou deltas acelerados — o resultado
seria aceleração aplicada duas vezes, e o ponteiro ficaria impossível de controlar em
movimentos rápidos. É um dos defeitos mais comuns nesta classe de software.

Quem mantém a posição corrente do ponteiro remoto é a sessão, em `ir-session`, com a
geometria de `ir-geometry`.

O agente **DEVE** declarar-se `PerMonitorV2` no manifesto, ou receberá coordenadas
virtualizadas em telas com escala diferente de 100%.

### 4.3. Ctrl+Alt+Del

`SendInput` não gera a Sequência de Atenção Segura — por projeto do Windows. O caminho é
`SendSAS(FALSE)` de `sas.dll`, chamado **pelo serviço**, e ele exige a política:

```text
HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System
    SoftwareSASGeneration = REG_DWORD
        0 = nenhum    1 = serviços    2 = apps de acessibilidade    3 = ambos
```

O instalador oferece isso como caixa **desmarcada**, com o texto explicando que a máquina
passa a aceitar Ctrl+Alt+Del gerado por software. A desinstalação reverte o valor se foi
o produto quem o alterou. Ver [04, §6](04-seguranca.md).

### 4.4. O endurecimento de janeiro de 2026 — a restrição que define o produto

**O fato.** A atualização de segurança de 13 de janeiro de 2026 (KB5073455 e da mesma
família; artigo KB5080542; CVE-2026-20824, com CVE-2026-20804 relacionada) mudou o
comportamento das interfaces de credencial do Windows. Elas passaram a **ignorar entrada
injetada** que não venha de uma origem confiável.

Pelas palavras da Microsoft, as origens aceitas são exatamente três:

1. **teclado físico**;
2. **aplicações de acessibilidade confiáveis, com o privilégio UIAccess** — o que exige
   binário assinado, instalado em local seguro e com a marca no manifesto;
3. **aplicações rodando com integridade elevada (administrador)**.

Tudo o mais é descartado em silêncio: teclas sintéticas de ferramentas de compartilhamento
de tela, teclados virtuais dentro de sessões remotas e automação que use `SendInput` ou
`PostMessage`. Gerenciadores de senha e leitores de tela foram quebrados por isso — o
KeePass registrou a falha de digitação automática no desktop seguro após a KB5074109, e a
recomendação prática que circulou foi exatamente rodar elevado ou adicionar
`uiAccess="true"` ao manifesto.

**Por que isto não mata o projeto — e por que quase matou.** O caminho ingênuo (um
programa comum do usuário chamando `SendInput`) está morto desde janeiro de 2026. O
desenho de três processos do [ADR-0001](adr/0001-tres-processos.md) coloca o agente como
`SYSTEM`, cuja integridade é *System*, acima de *High* (administrador elevado), e cujo
token contém `BUILTIN\Administrators`. Ele deve satisfazer o critério 3 pelas duas
leituras possíveis da regra.

Ou seja: a arquitetura escolhida por causa do desktop `Winlogon` é, por coincidência
feliz, a única que continua permitida. Um desenho de processo único e elevado teria sido
invalidado por uma atualização do Windows no meio do desenvolvimento.

**O que o produto DEVE fazer, em consequência.** Não basta contar com o critério 3.
O agente atende aos critérios 2 e 3 ao mesmo tempo:

| Exigência | Implementação |
|---|---|
| Integridade elevada | agente lançado como `SYSTEM` pelo serviço (§3.1) |
| Privilégio UIAccess | `SetTokenInformation(token, TokenUIAccess, 1)` antes do `CreateProcessAsUser` — exige `SeTcbPrivilege`, que o serviço tem |
| Binário assinado | **obrigatório**, não recomendado — UIAccess recusa binário sem assinatura válida |
| Local seguro | instalação em `%ProgramFiles%\InputRemote\`, gravável só por administradores |
| Manifesto | `<requestedExecutionLevel level="highestAvailable" uiAccess="true" />` no `inputremote-agent.exe` |

É o mesmo conjunto de medidas que o Deskflow adota no seu vigia do Windows, onde o token
de origem é duplicado e `TokenUIAccess` é marcado antes de criar o processo.

**A consequência dura:** assinatura de código deixa de ser boa prática e passa a ser
**requisito funcional**. Sem certificado, o pacote portátil continua servindo para uso
normal, mas **não digita na tela de bloqueio** — e isso precisa estar escrito no README,
não descoberto pelo usuário.

**O que ainda não sabemos, e que a PoC-1 tem de responder.** A documentação da Microsoft
fala em "diálogo de autenticação do Windows e interfaces de login", sem separar o que é
LogonUI na tela de boot, o que é a tela de bloqueio e o que é prompt de UAC dentro da
sessão. Relatos de campo sugerem impacto mais visível no UAC. Como as três superfícies são
requisito nosso, as três são testadas separadamente, em build já atualizado
([08, PoC-1](08-plano-de-implementacao.md)).

A Microsoft declarou estar trabalhando numa correção, sem prazo. O comportamento pode
mudar de novo — motivo pelo qual a PoC-1 é reexecutada a cada atualização cumulativa que
toque em autenticação, e o resultado é registrado com o número do build.

## 5. Captura, no lado servidor

Duas APIs, cada uma para o que faz bem:

| Necessidade | API |
|---|---|
| Deltas de mouse em alta resolução, sem aceleração do sistema | Raw Input (`WM_INPUT`) |
| Suprimir a entrada local enquanto o controle está no par | `WH_KEYBOARD_LL` e `WH_MOUSE_LL` |
| Teclas de sistema e combinações com modificador | `WH_KEYBOARD_LL` |

### 5.1. Regras do gancho de baixo nível

1. O procedimento do gancho **NÃO DEVE** fazer trabalho. Ele empilha numa fila sem
   alocação e retorna. O Windows remove ganchos que estouram `LowLevelHooksTimeout`
   (300 ms por padrão, em `HKCU\Control Panel\Desktop`) — **em silêncio**. O sintoma é
   "de repente parou de capturar" sem erro nenhum em lugar nenhum.
2. A thread do gancho **precisa** de um laço de mensagens próprio, e não pode ser a
   mesma que faz E/S.
3. Eventos com `LLKHF_INJECTED` / `LLMHF_INJECTED` são **ignorados**. Sem isso, quando as
   duas máquinas rodam o produto, a injeção de um lado é recapturada e volta — laço
   infinito de eventos.
4. Ganchos não veem Ctrl+Alt+Del, Win+L nem a troca para o desktop seguro. Ao detectar
   que o desktop de entrada mudou no **servidor**, o controle volta para local e um
   `ReleaseAll` é enviado ao par.

### 5.2. Prender o ponteiro local

Enquanto o controle está no par: `ClipCursor` para um retângulo de 1×1 no ponto de saída,
cursor oculto, e reposicionamento a cada evento. Ao voltar, `ClipCursor(NULL)` e o cursor
é restaurado na borda oposta.

`ClipCursor` é perdido em troca de desktop e em algumas janelas de tela cheia — o agente
o reaplica a cada 500 ms enquanto estiver em modo remoto.

## 6. Clipboard

`AddClipboardFormatListener` no agente (nunca *polling*), com `OpenClipboard` protegido
por repetição com espera curta — outra aplicação pode estar segurando o clipboard.

- texto: `CF_UNICODETEXT`, convertendo LF do protocolo para CRLF ao publicar;
- imagem: `CF_DIB`/`CF_DIBV5` na leitura, PNG canônico no protocolo, `CF_DIBV5` ao publicar;
- arquivos: `CF_HDROP`, materializados em pasta temporária antes de publicar.

Clipboard pertence ao agente do desktop `Default`. No desktop `Winlogon` não há clipboard
de usuário, e a sincronização fica suspensa — declaradamente, na interface.

> **Alterado pelo [ADR-0011](adr/0011-clipboard-na-travessia.md).** Quem cuida do clipboard não
> é o agente SYSTEM, e sim o ajudante `inputremote-agent --clipboard`, que roda **como o
> usuário** e fala pelo canal de controle. O clipboard é dado do usuário; não há motivo para ele
> passar por um processo com mais autoridade que isso.

## 7. Bluetooth dentro do serviço

Winsock, não WinRT. Ver [ADR-0005](adr/0005-bluetooth-rfcomm-winsock.md).

```text
socket(AF_BTH, SOCK_STREAM, BTHPROTO_RFCOMM)
SOCKADDR_BTH { btAddr, serviceClassId = UUID do InputRemote, port = 0 }
WSASetService(...)                       publica o registro SDP (lado que escuta)
WSALookupServiceBegin/Next(LUP_CONTAINERS)   inquérito de dispositivos
WSALookupServiceBegin/Next(sem LUP_CONTAINERS)   busca do serviço num dispositivo
```

`AF_BTH` é uma família de sockets do kernel e não depende de infraestrutura por usuário,
o que a torna adequada a um serviço na sessão 0. As chaves de pareamento Bluetooth do
Windows ficam em `HKLM\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Keys` — são
da máquina, então um par continua pareado na tela de login. Esse é o fato que torna o
requisito viável, e é o objeto da **PoC-2** ([08](08-plano-de-implementacao.md)).

## 8. Armadilhas conhecidas, com resposta

| Armadilha | Resposta |
|---|---|
| Gancho removido em silêncio por estouro de prazo | procedimento sem trabalho; vigia que reinstala o gancho se ele sumir |
| Injeção própria recapturada, gerando laço | filtrar `LLKHF_INJECTED` / `LLMHF_INJECTED` |
| Aceleração aplicada duas vezes no ponteiro | injeção sempre absoluta |
| Coordenadas erradas com telas de escalas diferentes | manifesto `PerMonitorV2` |
| `ClipCursor` perdido em tela cheia | reaplicar periodicamente enquanto remoto |
| Tecla presa após troca de desktop | estado no serviço + `StateSnapshot` após `AgentReady` |
| UIPI recusando injeção em janela elevada | agente é `SYSTEM`, integridade acima de qualquer janela |
| Interface de credencial descartando a injeção em silêncio (jan/2026) | agente `SYSTEM` **e** UIAccess **e** binário assinado em `%ProgramFiles%` — §4.4 |
| Build sem assinatura de código digitando normalmente, mas não na tela de bloqueio | limitação declarada no README; é consequência direta do UIAccess |
| Sessão 0 não enxerga o desktop de entrada da sessão 1 | quem vigia é o agente, não o serviço |
| Troca rápida de usuário deixa agente órfão | `WTS_CONSOLE_CONNECT` derruba todos e resubir |
| Windows Defender marcando o produto como *keylogger* | assinatura do binário e submissão para análise antes do lançamento |
| Retorno de suspensão com enlace morto e teclas presas | `PBT_APMSUSPEND` → `ReleaseAll` antes de dormir |
| `SetThreadDesktop` recusado por a thread já ter criado janela ou gancho | é a **primeira** instrução da thread; teste garante a ordem (§3.2) |
| Injetar no desktop errado sem erro nenhum | a thread de vigilância roteia; falha silenciosa é a pior categoria |
| Gancho de captura no desktop seguro | proibido por regra — seria ler a senha da própria máquina ([04, §4.1](04-seguranca.md)) |

A linha do Defender não é hipotética: um produto que instala ganchos globais, injeta
entrada e roda como serviço tem exatamente o perfil comportamental de um *keylogger*.
Assinar o binário e submeter à análise antes do lançamento é parte do trabalho, não
burocracia opcional.

## 9. Referências

Afirmações deste documento que dependem de fonte externa, para que possam ser reconferidas
quando o comportamento do Windows mudar.

| Assunto | Fonte |
|---|---|
| Endurecimento da interface de credencial (§4.4) | [Microsoft — New behavior restricting certain applications to autofill credentials (KB5080542)](https://support.microsoft.com/en-us/topic/new-behavior-restricting-certain-applications-to-autofill-credentials-introduced-by-the-windows-january-2026-security-update-29c0bc94-2588-41f9-8534-f058aa5214d5) |
| Atualização que introduziu o comportamento | [KB5073455 — 13 de janeiro de 2026 (build 22631.6491)](https://support.microsoft.com/en-us/servicing/os/windows-11/2026/01/january-13-2026-kb5073455-os-build-22631-6491) · [KB5074109 — builds 26200.7623 e 26100.7623](https://support.microsoft.com/en-US/servicing/os/windows-11/2026/01/january-13-2026-kb5074109-os-builds-26200-7623-and-26100-7623) |
| Vulnerabilidade tratada | [CVE-2026-20824](https://www.sentinelone.com/vulnerability-database/cve-2026-20824/) (relacionada: CVE-2026-20804) |
| Efeito prático em digitação automática no desktop seguro | [KeePass bug #2413](https://sourceforge.net/p/keepass/bugs/2413/) |
| Controle voltando ao servidor quando o cliente bloqueia | [deskflow#7899](https://github.com/deskflow/deskflow/issues/7899) · [deskflow#8183](https://github.com/deskflow/deskflow/issues/8183) |
| Necessidade do serviço para UAC e tela de login | [deskflow — Legacy FAQ](https://github.com/deskflow/deskflow/wiki/Legacy-FAQ) |
| Política de SAS por software | [deskflow — Workarounds](https://github.com/deskflow/deskflow/wiki/Workarounds) |
| Lançar processo interativo a partir de serviço | [Microsoft — Launching an interactive process from a Windows Service](https://learn.microsoft.com/en-us/archive/blogs/winsdk/launching-an-interactive-process-from-windows-service-in-windows-vista-and-later) · [CreateProcessAsUser](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw) |
| Restrição de `SwitchDesktop` em desktop seguro | [Microsoft — SwitchDesktop](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-switchdesktop) |
| Estrutura de window station e desktops | [Wikipédia — Winlogon](https://en.wikipedia.org/wiki/Winlogon) · [Microsoft — Winlogon and credential providers](https://learn.microsoft.com/en-us/windows/win32/secauthn/winlogon-and-credential-providers) |

Estado da bancada de referência em 09/09/2026: Windows 11 25H2, build **26200.9445** — já
inclui o endurecimento de janeiro de 2026, e portanto é um alvo válido para a PoC-1.
