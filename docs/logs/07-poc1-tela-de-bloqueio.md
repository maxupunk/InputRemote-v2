# PoC-1: o spike da tela de bloqueio, pronto para rodar

**Data:** 2026-09-09

**Itens:** Etapa 0, PoC-1 — todo o código. Os itens `[H]` continuam abertos: eles exigem uma
pessoa numa bancada Windows, e nenhum deles pode ser fechado daqui.

**O que foi feito.** `spikes/poc1-winlogon/`: um serviço e um agente mínimos que respondem a
pergunta bloqueante do produto — um agente `SYSTEM` com `TokenUIAccess`, numa thread amarrada
ao desktop `Winlogon`, consegue fazer `SendInput` chegar ao campo de senha num build com o
endurecimento de janeiro de 2026?

O spike está **fora do workspace** de propósito, com `[workspace]` próprio no manifesto. Ele
não carrega os lints nem os limites do produto, e o `xtask` ignora `spikes/` — código
descartável de prova de conceito não deve pagar o preço de código mantido
([08, §2](../08-plano-de-implementacao.md)). Confirmado: `cargo xtask check` continua vendo
65 arquivos.

### O que o spike implementa

O fluxo de [05, §3.1](../05-windows.md), no menor tamanho que ainda responde a pergunta:

1. `WTSGetActiveConsoleSessionId`, tratando `0xFFFFFFFF` como "ainda não há sessão" — o valor
   que a API devolve nos primeiros instantes do boot, e a diferença entre alcançar N3 e ficar
   em N2;
2. `OpenProcessToken` + `DuplicateTokenEx` para token primário;
3. `SetTokenInformation(TokenSessionId)`, que é onde `SeTcbPrivilege` é exigido;
4. `SetTokenInformation(TokenUIAccess)`, registrando a recusa em vez de abortar — a matriz de
   origem confiável quer justamente saber o que acontece sem ele;
5. `CreateProcessAsUserW` em `WinSta0\Default`, **não** em `Winlogon`: o desktop seguro é
   alcançado de dentro, por thread ([00b, §4](../00-licoes-do-deskflow.md)).

No agente: uma thread por desktop, cada uma chamando `SetThreadDesktop` como **primeira**
instrução; uma thread de vigilância consultando `OpenInputDesktop` a cada 200 ms e registrando
o atraso de detecção; e `SendInput` por scancode, para o layout da máquina controlada decidir o
caractere.

`SendSAS` é carregado dinamicamente de `sas.dll`. A ausência da biblioteca ou do símbolo é um
resultado a registrar, não um erro de ligação.

### O que os scripts protegem

**O instalador recusa build anterior ao endurecimento.** Compara `CurrentBuild.UBR` com
22631.6491 / 26100.7623 / 26200.7623 e aborta se for menor. Testar num build anterior é o erro
que faria a PoC **aprovar** e o produto falhar depois — o pior resultado possível, porque
esconde a informação em vez de revelá-la.

**Instala em `%ProgramFiles%` com ACL restrita.** `icacls` deixa só administradores e o
`SYSTEM` com escrita. Instalar na pasta do repositório faria o passo do `TokenUIAccess` passar
e o privilégio não ser concedido — um falso negativo silencioso.

**Avisa quando o binário não está assinado**, dizendo que aquilo é um resultado válido a
registrar mas não é a configuração (a) da matriz.

**Desinstala sem resíduo**, mas **preserva o registro**: apagar o log junto com a instalação
jogaria fora justamente o que se foi medir.

**Arquivos:** `spikes/poc1-winlogon/` — `Cargo.toml`, `README.md`, `src/{service,agent,log}.rs`,
`instalar.ps1`, `desinstalar.ps1`.

**Verificação.** Compila em release sem aviso, nos dois binários. `cargo xtask check` continua
limpo e continua ignorando `spikes/`. O comportamento em si **não foi verificado** — é o que
falta, e é o que ninguém pode fazer daqui.

**O que precisa de uma pessoa.** O `README.md` do spike traz o roteiro. O resultado não é
"passou" ou "falhou": é o **nível de capacidade do Windows** (N3, N2 ou N1) mais a matriz de
quatro configurações de token, com o número do build anotado. É essa matriz que diz qual
condição é de fato necessária, em vez de deduzir da documentação ambígua da Microsoft.
