# 04 — Segurança

## 1. O que este produto realmente é, do ponto de vista de segurança

Um serviço privilegiado que sobe antes do login, escuta um rádio Bluetooth
e uma porta UDP, e **digita no campo de senha da tela de bloqueio**.

Escrito assim, fica claro o que ele é: um caminho legítimo de contornar a autenticação
da máquina. Isso não é efeito colateral — é o requisito R1 de [01](01-visao-e-escopo.md).
A consequência é que este documento não é uma seção de encerramento; ele é uma restrição
de projeto tão dura quanto a latência.

Três frases guiam todo o resto:

1. Quem consegue falar com o serviço consegue digitar a senha de administrador.
2. Quem consegue reproduzir tráfego antigo consegue redigitar o que já foi digitado.
3. Quem consegue explorar o decodificador executa código como `SYSTEM`, remotamente, sem
   ninguém estar logado.

## 2. Modelo de ameaça

### Dentro do escopo

| Adversário | Capacidade | Controle |
|---|---|---|
| Vizinho de rede | envia pacotes UDP/TCP para a porta, escuta broadcast | Noise com chave estática fixada; sem chave, o handshake não passa da primeira mensagem |
| Escuta de rádio | grava tráfego Bluetooth e reproduz | contador explícito + janela deslizante de 2 048 (replay) |
| Homem no meio no primeiro pareamento | intercepta o primeiro encontro | código de 6 dígitos derivado do hash do handshake, comparado **visualmente** nos dois lados |
| Par legítimo comprometido | a outra máquina foi invadida | limites por par: permissão de tela de bloqueio, de arquivos e de clipboard são separadas e revogáveis |
| Usuário local sem privilégio | tenta usar o IPC para digitar como SYSTEM | autorização do IPC (§5) — é o vetor mais provável e mais subestimado |
| Pacote malformado | explora o decodificador | Rust sem `unsafe` no decodificador + fuzzing obrigatório no CI |
| Máquina roubada e pareada | outra máquina pareada digita a senha remotamente | política de tela de bloqueio opcional por par (§6) |

### Fora do escopo, declarado

- Adversário com privilégio administrativo já obtido na máquina. Ele pode desinstalar o
  serviço; não há defesa possível nem pretendida.
- Ataque físico ao rádio Bluetooth em camada de banda base.
- Acesso pela internet, relay e travessia de NAT — não existem no produto.
- Confidencialidade contra a própria máquina par: um par legítimo vê o que é digitado
  nele, por definição.

## 3. Identidade e pareamento

### 3.1. Identidade

Cada máquina gera, na primeira subida do serviço, um par estático X25519. A chave privada
nunca sai da máquina e é gravada com ACL `SYSTEM` + `Administrators` (Windows) ou
`0600 root:root` (Linux). Ela é da **máquina**, não do usuário — o serviço precisa dela
antes de haver usuário.

Impressão digital exibida ao usuário: BLAKE3 da chave pública, em 8 grupos de 4
caracteres da base32 sem ambiguidade.

### 3.2. Primeiro pareamento

Não há PAKE e não há nada para o usuário digitar. O modelo é o mesmo do ZRTP e do Signal:
*short authentication string*.

```text
 A                                                        B
 │  usuário escolhe B na lista e pede "parear"            │
 │                                                        │
 │──── Noise_XX, sem autenticação prévia ────────────────►│
 │◄──────────────────────────────────────────────────────-│
 │                                                        │
 │  cada lado deriva SAS = 6 dígitos do hash do handshake │
 │  (h do Noise, via HKDF, rótulo "inputremote-sas-v1")   │
 │                                                        │
 │      A mostra: 418 902        B mostra: 418 902        │
 │                                                        │
 │  o usuário olha as DUAS telas e confirma nos DOIS lados│
 │                                                        │
 │  → cada lado grava a chave estática pública do outro   │
```

Por que isso basta: um atacante no meio precisa fazer dois handshakes distintos, um com
cada lado, e o hash de cada um é diferente — logo os dois códigos exibidos serão
diferentes, e o usuário vê. Ele teria uma chance em um milhão de acertar por tentativa, e
as chaves efêmeras são novas a cada tentativa, então não há ataque acumulativo nem
possibilidade de força bruta *offline*.

Regras que sustentam a garantia:

- a confirmação visual dos dois lados **NÃO DEVE** ser pulável, nem por configuração;
- o handshake expira em 2 minutos sem confirmação;
- nenhum dado da sessão trafega antes das duas confirmações;
- 5 tentativas de pareamento por par, com espera crescente entre elas;
- a comparação do código recebido é feita em tempo constante.

### 3.3. Reconexões

`Noise_IK` com a chave estática do par **fixada**. Chave diferente da fixada é recusa,
não é pergunta ao usuário. Não há "confiar na primeira vez" depois do pareamento.

Isto corrige diretamente o item que ficou aberto no v1: *"identidade do par fixada após
pareamento — [ ]"*. Um KVM que reconecta sem verificar identidade forte é um KVM que
aceita qualquer um que chegue primeiro.

### 3.4. Revogação

Remover um par é uma ação da interface que exige elevação (§5) e apaga a chave fixada.
A partir daí, aquele par precisa de novo código e nova confirmação visual.

## 4. Redução de privilégio

O serviço precisa de privilégio para injetar na tela de bloqueio. Ele não precisa dele
para decodificar pacotes. A arquitetura reflete isso:

| Componente | Privilégio | Por quê |
|---|---|---|
| `inputremote-daemon` (Windows) | `SYSTEM` | lançar agente no desktop `Winlogon`; `SendSAS` |
| `inputremote-daemon` (Linux) | usuário de sistema `inputremote` | escrever em `/dev/uinput`; falar com o BlueZ |
| `inputremote-agent` (Windows) | `SYSTEM` + UIAccess, na sessão de console | injetar no desktop seguro (§4.1) |
| `inputremote-agent` (Linux) | usuário da sessão | clipboard e geometria, nada privilegiado |
| `inputremote-ui` | usuário, sem elevação | só desenha e fala IPC |

No Linux o serviço **não precisa ser `root`**, e não deve ser. O acesso a `/dev/uinput` é
governado por permissão de arquivo, não por capacidade — então basta um usuário de sistema
dedicado com `DeviceAllow=/dev/uinput rw` e uma política de D-Bus que permita registrar o
perfil no BlueZ. Nenhuma capacidade é mantida: `CapabilityBoundingSet=` vazio,
`NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes` e
`SystemCallFilter=@system-service` ([06, §7](06-linux.md)).

Se a PoC-3 mostrar que o registro do perfil BlueZ exige `root`, o serviço sobe como `root`
e larga o privilégio depois de abrir `/dev/uinput` e o barramento — a ordem correta fica
decidida pela PoC, não por suposição.

### 4.1. O caso especial do agente no Windows

O agente do Windows é a exceção: além de `SYSTEM`, ele carrega o privilégio **UIAccess** e
precisa ser um binário **assinado**, instalado em local gravável apenas por administradores.

Isso não é zelo. Desde a atualização de segurança de janeiro de 2026, as interfaces de
credencial do Windows descartam entrada injetada que não venha de teclado físico, de
aplicação com UIAccess ou de aplicação com integridade elevada
([05, §4.4](05-windows.md)). Sem essas três propriedades juntas, o requisito R1
simplesmente não acontece — e falha em silêncio, sem erro em lugar nenhum.

A consequência de segurança precisa estar dita: o produto pede ao usuário que instale um
binário assinado, com UIAccess, que roda como `SYSTEM` e digita na tela de bloqueio. É
exatamente o perfil de uma ferramenta de ataque. A defesa não é o produto ser inofensivo —
ele não é — e sim ser auditável, assinado, de código aberto, com o controle de acesso do
§5 funcionando.

Daí uma regra que limita o próprio produto:

> **O agente NÃO DEVE instalar gancho de captura no desktop `Winlogon`.**
> Injetar ali é o requisito; **ler** dali seria registrar o que se digita na tela de
> bloqueio da própria máquina — a definição de keylogger, e sem finalidade nenhuma, já que
> quando o servidor bloqueia o controle volta para local de qualquer forma.

A regra é verificável: o código que instala `WH_KEYBOARD_LL` fica num módulo usado apenas
pela thread do desktop `Default`, e um teste falha se ele for alcançável das outras
([ADR-0008](adr/0008-agente-com-thread-por-desktop.md)).

No Windows, o serviço declara `RequiredPrivileges` mínimo — `SeTcbPrivilege`,
`SeAssignPrimaryTokenPrivilege`, `SeIncreaseQuotaPrivilege` — e nada além.

**Meta arquitetural, Fase 2:** mover o decodificador de rede para um processo separado,
sem privilégio, que entrega ao serviço apenas mensagens já validadas. É a evolução
natural, e a fronteira de crates de [02](02-arquitetura.md) já foi desenhada para
permitir isso sem reescrita.

## 5. Autorização do IPC — o vetor mais provável

Se qualquer processo do usuário puder mandar `Inject(KeyDown)` para o serviço, então
qualquer programa que o usuário rodar pode digitar no prompt de UAC. Isso destruiria o
modelo de segurança do Windows na máquina.

Regras:

**Windows.** Named pipe `\\.\pipe\inputremote-control`, com descritor de segurança
explícito. Cada conexão passa por `ImpersonateNamedPipeClient` e o serviço lê o token do
chamador. Três níveis:

| Operação | Exigência |
|---|---|
| Ler estado, ler configuração, diagnóstico | usuário interativo da sessão de console |
| Alterar configuração, iniciar e parar sessão | usuário interativo da sessão de console |
| Parear, remover par, permitir tela de bloqueio, habilitar SAS | **elevação** — token com `Administrators` habilitado |
| Injetar entrada | **somente o agente**, autenticado por token `SYSTEM` e por segredo de uma via entregue na criação do processo |

**Linux.** Socket `/run/inputremote/control.sock`. Quem decide o acesso é o **serviço**, e não a
permissão do arquivo: a cada conexão ele lê a identidade do chamador por `SO_PEERCRED` e consulta
no banco de usuários (`getgrouplist`) se esse usuário pertence ao grupo `inputremote` **naquele
momento**. Por isso o socket de controle é alcançável por qualquer processo local (`0666`) — o
portão é a credencial — e o do agente é `0600 root`. root e o usuário dono do serviço sempre
entram; quem é recusado recebe `SemPermissao` como resposta, e não um fechamento mudo. As
operações da terceira linha exigem `polkit` (`org.inputremote.pair`,
`org.inputremote.lockscreen`).

> O controle por permissão de arquivo (`0660 root:inputremote`) foi o primeiro desenho
> implementado e **NÃO DEVE** voltar. A permissão de arquivo é conferida com os grupos que o
> processo carrega, e no GNOME a sessão gráfica guarda os grupos do login até a máquina
> reiniciar: um `usermod` ficava certo no banco e não chegava à janela
> ([log 17](logs/17-a-janela-que-volta-e-o-grupo-que-vale-na-hora.md)).

O canal do agente é **separado** do canal da interface — pipe e socket distintos, com
permissões distintas. A interface nunca pode mandar `Inject`, em nenhuma circunstância.

## 6. Política de tela de bloqueio

Digitar na tela de bloqueio é opcional e granular. Padrão: **desligado**.

- opção da instalação: "permitir controle na tela de bloqueio e em prompts de elevação";
- opção **por par**: cada máquina pareada tem essa permissão concedida separadamente;
- opção independente: "permitir Ctrl+Alt+Del" (exige a política `SoftwareSASGeneration`
  do Windows — ver [05](05-windows.md));
- quando desligada, o cliente ignora entrada enquanto o desktop for `Winlogon` e informa
  ao servidor, que mostra na interface por que o teclado parou de funcionar. Ele **NÃO
  DEVE** falhar em silêncio — foi assim que o v1 gerou perguntas sem resposta.

Controle adicional recomendado, ligado por padrão: **o cliente só aceita entrada na tela
de bloqueio se o servidor declarar que está desbloqueado**. Se o notebook do servidor foi
roubado bloqueado, ele não vira uma chave para o desktop bloqueado do lado de cá.

## 7. Privacidade dos logs

O produto vê tudo o que é digitado, incluindo senhas. Regras absolutas:

- **NUNCA** registrar conteúdo de tecla, caractere, HID Usage ou coordenada em nível
  `info` ou acima;
- o nível `trace` de entrada existe, mas exige recompilação com a *feature*
  `unsafe-input-logging`, que **NÃO DEVE** estar habilitada em nenhum artefato publicado;
- clipboard: registra-se tipo e tamanho, nunca conteúdo;
- nomes de arquivo em transferência: registrados em `debug`, com opção de anonimizar;
- o relatório de diagnóstico exportável é montado por uma função dedicada, com lista de
  campos permitidos (*allowlist*), nunca por filtro de exclusão;
- segredos em memória usam `zeroize` e são apagados na queda da sessão.

## 8. Cadeia de suprimentos

- `cargo-deny` no CI: licenças permitidas, avisos de segurança do RustSec, dependências
  duplicadas e fontes não confiáveis;
- `cargo-vet` ou `cargo-audit` obrigatório antes de cada lançamento;
- dependências fixadas com `Cargo.lock` versionado, inclusive para binários;
- toda dependência nova entra por decisão registrada em [07](07-stack-e-dependencias.md),
  com justificativa de por que não dá para viver sem ela;
- compilações de lançamento são reproduzíveis, e cada artefato publicado leva `.sha256`
  e assinatura.

## 9. Instalação e desinstalação

- o instalador e **todos os binários** do Windows **DEVEM** ser assinados. São duas razões
  independentes: distribuir um serviço `SYSTEM` sem assinatura é irresponsável, e o
  UIAccess do agente **não funciona** sem assinatura válida (§4.1). Assinatura passou de
  boa prática a requisito funcional;
- uma build sem assinatura é utilizável para desenvolvimento e para uso normal, mas
  **não digita na tela de bloqueio**; isso **DEVE** estar no README, não ser descoberto
  pelo usuário;
- a desinstalação **DEVE** remover o serviço, o dispositivo `uinput`, as regras `udev`,
  as chaves e a política `SoftwareSASGeneration` se ela tiver sido alterada pelo produto;
- a instalação **NÃO DEVE** alterar a política de SAS sem consentimento explícito e
  informado, numa tela que diga o que muda no comportamento do sistema.
