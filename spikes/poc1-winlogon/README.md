# PoC-1 — Digitar na tela de bloqueio do Windows

⚠ **Esta prova de conceito é bloqueante do produto inteiro.** Se ela reprovar por completo,
o InputRemote 2 precisa ser redefinido antes de qualquer linha de código de produção.
Ver [docs/08-plano-de-implementacao.md](../../docs/08-plano-de-implementacao.md) §2.

Código **descartável**. Está fora do workspace de propósito: não carrega os lints nem os
limites do produto, porque existe para responder uma pergunta, não para ser mantido.

## A pergunta

Um serviço `LocalSystem` consegue lançar um agente na sessão de console e, de uma thread dele
amarrada ao desktop `WinSta0\Winlogon`, injetar teclas que cheguem ao campo de senha — **num
build com o endurecimento de janeiro de 2026**?

Desde a KB5073455, as interfaces de credencial do Windows só aceitam entrada de três origens:
teclado físico, aplicação com UIAccess, ou aplicação com integridade elevada
([docs/05 §4.4](../../docs/05-windows.md)). O agente é `SYSTEM`, cuja integridade é *System*,
acima de *High* — ele **deve** se enquadrar. Mas "deve" não é "se enquadra", e é isso que se
vai medir.

## Ambiente obrigatório

| | Exigência |
|---|---|
| Build do Windows | **≥ 26100.7623 / 26200.7623 / 22631.6491** |
| Privilégio | administrador, para instalar o serviço |
| Assinatura | certificado de teste, para a matriz completa |
| Local | `%ProgramFiles%\poc1\`, para o UIAccess ser concedido |

Confira o build antes de qualquer coisa. Testar num build anterior ao endurecimento **invalida
o resultado** — é o erro que faria a PoC aprovar e o produto falhar depois.

```powershell
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
"$($cv.CurrentBuild).$($cv.UBR)"
```

## Como rodar

```powershell
# 1. Compilar
cargo build --release

# 2. Instalar (como administrador). Copia para %ProgramFiles%\poc1 e registra o serviço.
.\instalar.ps1

# 3. Bloquear a tela e esperar. O agente digita a sequencia 20 s depois de detectar
#    a troca para o desktop Winlogon.
rundll32.exe user32.dll,LockWorkStation

# 4. Ler o resultado
Get-Content "$env:ProgramData\poc1\poc1.log"

# 5. Desinstalar
.\desinstalar.ps1
```

## O que registrar

O resultado desta PoC **não** é "passou" ou "falhou". É o **nível de capacidade do Windows**,
com o número do build anotado:

| Itens 1, 2 e 4 | Item 3 | Nível |
|---|---|---|
| passam | passa | **N3** — alvo alcançado |
| passam | falha | **N2** — aceitável; a tela de login exige teclado físico uma vez |
| falham | — | **N1** — o requisito R1 não foi atendido no Windows |

Os itens, em ordem:

1. a sequência aparece no campo de senha da **tela de bloqueio**;
2. a máquina desbloqueia com a senha digitada remotamente;
3. o mesmo funciona na **tela de login logo após o boot**, sem sessão de usuário;
4. o mesmo funciona num **prompt de UAC**;
5. a troca `Default` → `Winlogon` é detectada em menos de 300 ms;
6. `SendSAS` produz a tela de Ctrl+Alt+Del com a política habilitada.

E a **matriz de origem confiável**, que é o que diz qual condição é de fato necessária em vez
de deduzir da documentação ambígua da Microsoft. Rode os itens 1, 3 e 4 em cada configuração:

| # | Configuração do agente | Resultado |
|---|---|---|
| a | `SYSTEM` + UIAccess + assinado, em `%ProgramFiles%` | |
| b | `SYSTEM`, sem UIAccess | |
| c | elevado como administrador, sem `SYSTEM` | |
| d | usuário comum | |

Escolha a configuração com `POC1_MODE` no ambiente do serviço: `system-uiaccess` (padrão),
`system`, `elevated` ou `user`.

## Interpretando o resultado

- **Nível N2.** Nada é bloqueado. Registra-se a limitação no README do produto e na interface,
  e o desenvolvimento segue. Era a decisão tomada de antemão, e não se renegocia com o
  resultado na mão.
- **Nível N1.** O produto precisa ser redefinido: nem a tela de bloqueio funciona, e o serviço
  privilegiado perde a razão de existir. A alternativa a investigar é driver HID virtual, que
  entra como teclado físico — ao custo de assinatura WHQL com certificado EV.
- **Reprovou só (b), (c) ou (d).** Nada muda: é a confirmação de que (a) é obrigatória, e ela
  já é a configuração do projeto.

Anote o resultado em [LOG.md](../../LOG.md) e marque os itens em
[PROGRESSO.md](../../PROGRESSO.md). Esta PoC é reexecutada a cada atualização cumulativa que
toque em autenticação — a Microsoft declarou estar trabalhando numa correção do comportamento,
sem prazo, e o alvo pode se mover nas duas direções.
