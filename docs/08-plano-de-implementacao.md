# 08 — Plano de implementação

## 1. A regra que governa este plano

> **Nenhuma etapa começa sem que a prova de conceito correspondente tenha passado em
> hardware real, com o critério de aprovação escrito antes do teste.**

O v1 produziu 1.142 linhas de backend Bluetooth contra um comportamento nunca observado,
e fechou a versão 0.1.0 com "Prova RFCOMM Windows↔Windows — [ ]" em aberto. O custo disso
não é o código jogado fora: é que as decisões arquiteturais tomadas em cima da suposição
já haviam contaminado o resto.

Uma PoC reprovada muda a arquitetura, não o cronograma.

Não há MVP. Cada etapa é entregue completa, validada e sem pendência declarada como
"depois". Uma etapa com item aberto bloqueia a próxima.

## 2. Etapa 0 — Provas de conceito

Código descartável, fora do repositório do produto, em `spikes/`. O objetivo é responder
perguntas, não construir. Duração estimada: 2 a 3 semanas.

### PoC-1 — Digitar na tela de bloqueio do Windows ⚠ bloqueante do produto inteiro

**Pergunta.** Um serviço `LocalSystem` consegue lançar um agente na sessão de console e,
de uma thread dele amarrada ao desktop `Winlogon`, injetar teclas que cheguem ao campo de
senha — **em um build com o endurecimento de janeiro de 2026**? E detecta a troca de
desktop a tempo?

A segunda metade da pergunta é nova e é a mais perigosa. Desde a KB5073455, as interfaces
de credencial só aceitam entrada de teclado físico, de aplicação com UIAccess ou de
aplicação com integridade elevada ([05, §4.4](05-windows.md)). O agente `SYSTEM` **deve**
se enquadrar, mas "deve" não é "se enquadra".

**Como.** Serviço mínimo em Rust + agente mínimo, com o fluxo de
[05, §3.1](05-windows.md), incluindo `TokenUIAccess`, e com a estrutura de threads de
[ADR-0008](adr/0008-agente-com-thread-por-desktop.md) — thread do `Winlogon` criada na
subida, com `SetThreadDesktop` como primeira instrução. O binário do agente é assinado com
certificado de teste e instalado em `%ProgramFiles%`. Um temporizador que, 20 segundos
após o `Win+L`, digita uma sequência conhecida.

**Ambiente.** Build **igual ou posterior a** 26100.7623 / 26200.7623 / 22631.6491, com o
número anotado. Testar num build anterior invalida o resultado.

**Aprovação.**
1. a sequência aparece no campo de senha da **tela de bloqueio**;
2. a máquina desbloqueia com a senha digitada remotamente;
3. o mesmo funciona na **tela de login logo após o boot**, sem sessão de usuário;
4. o mesmo funciona num **prompt de UAC**;
5. `OpenInputDesktop` a partir do agente detecta a troca `Default` → `Winlogon` em menos
   de 300 ms, e também o retorno;
6. `SendSAS` produz a tela de Ctrl+Alt+Del com a política habilitada;
7. **matriz de origem confiável:** repetir os itens 1, 3 e 4 em quatro configurações do
   agente — (a) `SYSTEM` + UIAccess + assinado, (b) `SYSTEM` sem UIAccess, (c) elevado
   como administrador, (d) usuário comum. O resultado de cada uma é registrado. É isto que
   diz qual condição é realmente necessária, em vez de deduzir da documentação.

Os itens 1, 3 e 4 são superfícies diferentes e a documentação da Microsoft não as separa.
Uma delas falhar não invalida as outras — mas muda o produto, e o que muda precisa estar
escrito antes de qualquer código de produção.

**Resultado.** A PoC **não** devolve "passou" ou "falhou". Ela devolve o **nível de
capacidade alcançado no Windows** ([01, §2](01-visao-e-escopo.md)), com o número do build:

| Itens 1, 2 e 4 | Item 3 | Nível |
|---|---|---|
| passam | passa | **N3** — alvo alcançado |
| passam | falha | **N2** — aceitável; a tela de login exige teclado físico uma vez |
| falham | — | **N1** — o requisito R1 não foi atendido no Windows |

**Se o resultado for N2.** Nada é bloqueado. Registra-se a limitação no README e na
interface, e o desenvolvimento segue. Era a decisão tomada de antemão, e não se
renegocia depois com o resultado na mão.

**Se o resultado for N1.** Aí sim o produto precisa ser redefinido antes de qualquer outra
coisa, porque nem a tela de bloqueio funciona e o serviço privilegiado perde a razão de
existir. Alternativa a investigar nesse caso: driver HID virtual, que entra como teclado
físico — a origem confiável número 1 da lista da Microsoft — ao custo de assinatura WHQL
com certificado EV, submissão a cada versão, e um projeto diferente do que está aqui
documentado.

**Se reprovar só (b), (c) ou (d) da matriz.** Nada muda: é a confirmação de que a
configuração (a) é obrigatória, e ela já é a do projeto.

**Reexecução.** Esta PoC é reexecutada a cada atualização cumulativa do Windows que toque
em autenticação, e o resultado é registrado com o número do build. A Microsoft declarou
estar trabalhando numa correção do comportamento, sem prazo — o alvo pode se mover nas
duas direções.

### PoC-2 — Bluetooth RFCOMM dentro de um serviço ⚠ bloqueante do portador principal

**Pergunta.** `AF_BTH` + `BTHPROTO_RFCOMM` funciona a partir da sessão 0? O enlace sobe
antes do login? Qual é a latência real?

**Como.** Dois computadores pareados **manualmente** pelas configurações do sistema.
Serviço Windows publicando o serviço por `WSASetService`; do outro lado, Windows e depois
Linux com `bluer`.

**Aprovação.**
1. o socket abre a partir do serviço, na sessão 0, sem usuário logado;
2. o par continua pareado depois de reiniciar as duas máquinas;
3. Windows↔Windows e Windows↔Linux, nos dois sentidos de quem escuta;
4. RTT com carga de 125 mensagens por segundo, por 10 minutos: mediana < 20 ms,
   p99 < 50 ms;
5. reconexão automática em menos de 5 s após desligar e religar o rádio;
6. MTU efetiva de RFCOMM medida e registrada nas duas pilhas;
7. **comparação lado a lado** com UDP em Ethernet cabeada e com UDP em Wi-Fi, na mesma
   bancada, na mesma sessão de medição.

O item 7 existe porque a premissa "Bluetooth por causa da latência" precisa ser verificada,
não assumida. A expectativa honesta, antes de medir:

| Portador | Mediana esperada | p99 esperado |
|---|---|---|
| UDP em Ethernet cabeada | < 1 ms | < 2 ms |
| Bluetooth RFCOMM | 10 a 30 ms | pior sob interferência de 2,4 GHz |
| UDP em Wi-Fi congestionada | 2 a 5 ms | 20 a 50 ms, com picos |

Se isso se confirmar, **Bluetooth perde feio para cabo e ganha do Wi-Fi ruim** — e a
vantagem real dele passa a ser outra: não depender de rede nenhuma, e não disputar o rádio
Wi-Fi com o resto da casa. Isso continua sendo um bom motivo, mas é um motivo diferente do
que está escrito hoje em [01, §5](01-visao-e-escopo.md), e a política de escolha de
portador precisa refletir o que foi medido.

**Se reprovar (só a sessão 0).** Investigar Bluetooth a partir do agente, com o serviço
fazendo a ponte — perde-se a tela de bloqueio por Bluetooth, mas ela sobrevive por UDP.

**Se reprovar (latência).** O Bluetooth deixa de ser o portador preferido e vira
alternativa; UDP passa a ser o principal. É uma mudança de produto, e precisa ser tomada
aqui, não depois de 4.000 linhas escritas.

### PoC-3 — `uinput` no greeter e na tela de bloqueio do Linux

**Pergunta.** O daemon consegue digitar a senha no GDM e na tela de bloqueio do GNOME?
Qual é o atraso real entre `UI_DEV_CREATE` e o primeiro evento aceito? E o serviço
consegue rodar sem ser `root`?

**Aprovação.**
1. digitar a senha na **tela de bloqueio** do GNOME e do KDE e desbloquear (N2);
2. digitar a senha no **greeter do GDM**, após o boot, e entrar (N3);
3. atraso de enumeração após `UI_DEV_CREATE` medido e registrado;
4. funcionar com SELinux em *enforcing* no Fedora;
5. ponteiro absoluto acertando o pixel pedido em telas com escalas diferentes;
6. os itens 1 e 2 funcionando com o serviço rodando como usuário de sistema dedicado,
   **sem** `root` — inclusive o registro do perfil no BlueZ.

**Resultado.** Como a PoC-1, devolve o **nível alcançado no Linux**: itens 1 e 2 passam →
N3; só o item 1 → N2; nenhum → N1.

**Se o item 6 reprovar.** O serviço sobe como `root` e larga o privilégio depois de abrir
`/dev/uinput` e o barramento. É a única linha de [04, §4](04-seguranca.md) que muda.

### PoC-4 — `InputCapture` + `libei` como servidor

**Pergunta.** As barreiras disparam com precisão suficiente? O `restore_token` evita o
segundo diálogo de permissão?

**Aprovação.** Barreira nas quatro bordas, com múltiplos monitores, no GNOME e no KDE;
eventos chegando por `libei` com atraso mediano abaixo de 5 ms; segunda execução sem
nenhum diálogo; comportamento definido e legível quando o compositor não tem o portal.

### PoC-5 — Noise sobre os três portadores

**Pergunta.** Uma camada de criptografia só atende stream e datagrama sem gambiarra? Qual
o custo por mensagem?

**Aprovação.** `Noise_XX` com código derivado do handshake, e `Noise_IK` com chave fixada,
funcionando sobre RFCOMM, UDP e TCP com o mesmo código; janela de repetição rejeitando
reenvio; custo de cifrar e decifrar uma mensagem de entrada abaixo de 20 µs; latência
adicionada em UDP na LAN com mediana < 8 ms.

### PoC-6 — Empacotamento

**Aprovação.** Um comando no Windows gera instalador assinado, ZIP portátil, RPM e DEB.
Instalar e desinstalar não deixa resíduo: serviço, regras `udev`, chaves e política de SAS.

## 3. Etapas do produto

Cada etapa tem critério de entrada e de saída. Sem o de saída cumprido, a próxima não
começa.

### Etapa 1 — Esqueleto

Workspace, os três binários, IPC nos dois sentidos, logs, configuração, instalação e
desinstalação nos dois sistemas.

**Saída.** O serviço sobe no boot das duas plataformas; a interface abre, mostra "sem par"
e a impressão digital da máquina; instalar e desinstalar é limpo; o CI roda `fmt`,
`clippy`, `deny` e as regras de [09](09-padroes-de-codigo.md).

### Etapa 2 — Núcleo sem E/S

`ir-proto`, `ir-geometry` e `ir-session` completos, com o catálogo de mensagens de
[03](03-protocolo.md) e a máquina de estados inteira.

**Saída.** Cobertura acima de 85% nesses três crates; travessia de borda, reconexão,
troca de portador, `StateSnapshot` e liberação de teclas testados sem hardware nenhum;
fuzzing do decodificador rodando no CI sem achados.

### Etapa 3 — Criptografia e pareamento

`ir-crypto`, com `Noise_XX` + código visual, `Noise_IK` com chave fixada, janela de
repetição, rechaveamento e armazenamento das chaves.

**Saída.** Pareamento entre duas máquinas pela rede; chave trocada é recusada; teste
automatizado de repetição e de handshake adulterado.

### Etapa 4 — Rede

`ir-net`: UDP de entrada com a confiabilidade de [03, §4.1](03-protocolo.md), TCP de
dados, descoberta mDNS.

**Saída.** Sessão completa por UDP entre duas máquinas; perda de 5% injetada
artificialmente não produz tecla presa; latência medida dentro da meta de
[01, §6](01-visao-e-escopo.md).

### Etapa 5 — Entrada no Windows

Captura, supressão, injeção, ciclo de vida do agente, troca de desktop, tela de bloqueio,
`SendSAS`.

**Saída.** Windows→Windows completo, com o **nível de capacidade da PoC-1 confirmado no
produto** — no mínimo N2: digitar a senha na tela de bloqueio e operar um prompt de UAC;
troca de desktop instantânea, sem criação de processo; teste de estresse de 10.000
travessias sem tecla presa; agente morto à força ressobe em menos de 500 ms sem deixar
estado sujo; nenhum gancho instalado no desktop `Winlogon`, verificado por teste.

### Etapa 6 — Entrada no Linux

`uinput` para injeção, `InputCapture` + `libei` para captura, integração com `logind`.

**Saída.** As quatro combinações entre as duas plataformas funcionando; **nível de
capacidade da PoC-3 confirmado no produto**, no mínimo N2 (tela de bloqueio do GNOME e do
KDE); mesmo teste de estresse aprovado.

### Etapa 7 — Bluetooth

`ir-bt` com os dois backends, seleção de portador pela política única de
[01, §5](01-visao-e-escopo.md), reconexão.

**Saída.** As quatro combinações por Bluetooth; queda do rádio degrada para UDP conforme
a política, com o motivo visível; latência dentro da meta.

### Etapa 8 — Clipboard e arquivos

`ir-clip` e `ir-files`: texto, imagem, arquivos, manifesto, cotas, BLAKE3, progresso,
cancelamento.

**Saída.** Transferência de 5 GB sem degradar a latência da entrada além de 10%;
cancelamento no meio não deixa arquivo temporário nem árvore parcial; falha de rede não
interrompe teclado e mouse por Bluetooth.

### Etapa 9 — Interface

Slint: configuração, pareamento com o código visual, estado, diagnóstico, bandeja.

**Saída.** Todo estado observável de [01, §5](01-visao-e-escopo.md) visível; fechar,
matar ou nunca abrir a interface não altera a sessão em nada.

### Etapa 10 — Qualidade e lançamento

Assinatura, instaladores, documentação de usuário, relatório de diagnóstico, roteiro de
validação física.

**Saída.** O critério de pronto de [01, §7](01-visao-e-escopo.md), integralmente.

## 4. Riscos, com gatilho e resposta

Um risco sem gatilho observável é um desejo. Cada linha diz o que dispara a resposta.

| Risco | Gatilho | Resposta |
|---|---|---|
| Tela de **login** inalcançável (N2 em vez de N3) | PoC-1 item 3, ou PoC-3 item 2 | entrega-se N2 naquela plataforma, com a limitação declarada; **não bloqueia o lançamento** |
| Tela de **bloqueio** inalcançável (N1) | PoC-1 itens 1/2/4 reprovam | redefinir o produto antes de escrever qualquer código — o serviço privilegiado perde a razão de existir |
| Bluetooth inacessível da sessão 0 | PoC-2 item 1 reprova | Bluetooth pela ponte do agente; tela de bloqueio só por UDP |
| Latência do Bluetooth pior que a da rede | PoC-2 item 4 reprova | UDP vira portador preferido; Bluetooth vira alternativa |
| Compositor sem `InputCapture` | detecção em tempo de execução | sem papel de servidor ali; papel de cliente intacto |
| Antivírus classificando como *keylogger* | primeiro envio para análise | assinatura, submissão antecipada, documentação do comportamento |
| Assinatura de código indisponível | Etapa 10 | pacote portátil funciona para uso normal, **mas não digita na tela de bloqueio** — UIAccess exige binário assinado; limitação no README |
| Windows endurecer mais a interface de credencial | PoC-1 reexecutada a cada atualização de autenticação | reavaliar; o caminho restante seria driver HID virtual |
| Diferença de layout de teclado | testes da Etapa 5 | HID Usage no protocolo; layout decidido no lado controlado |
| Crescimento descontrolado de arquivo ou crate | CI de [09](09-padroes-de-codigo.md) | falha de build, não aviso |
| `libei` ou portais mudando de contrato | CI noturno contra as versões alvo | fixar versões e testar em contêiner |
