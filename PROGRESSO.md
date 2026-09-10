# Progresso

Checklist verificável da implementação. Espelha [docs/08-plano-de-implementacao.md](docs/08-plano-de-implementacao.md).

## Regras deste arquivo

1. Um item só recebe `[x]` quando estiver **implementado e verificado** — por teste que
   passa, build que compila ou inspeção funcional registrada. Não existe `[x]` por
   "acredito que funciona".
2. Todo `[x]` **exige uma entrada de log correspondente**: um arquivo em
   [`docs/logs/`](docs/logs/), indexado em [LOG.md](LOG.md), com data, o que foi feito, arquivos
   tocados e como foi verificado.
3. `[~]` significa em andamento. `[!]` significa bloqueado — e o motivo fica escrito ao lado.
4. `[H]` significa que só pode ser verificado em hardware físico, por uma pessoa. O código
   pode estar pronto; o item não fecha sem a execução.
5. Uma etapa com item aberto **bloqueia a próxima**, conforme a regra do plano. A exceção
   registrada é a Etapa 2, que roda em paralelo à Etapa 0 porque não depende de hardware.

Legenda: `[ ]` pendente · `[~]` em andamento · `[x]` feito e verificado · `[!]` bloqueado ·
`[H]` aguardando execução em hardware

---

## Etapa 0 — Provas de conceito

### PoC-1 — Digitar na tela de bloqueio do Windows ⚠ bloqueante do produto
- [x] Serviço mínimo `LocalSystem` que registra, sobe e para
- [x] Lançamento de agente na sessão de console com `TokenUIAccess`
- [x] Thread por desktop com `SetThreadDesktop` como primeira instrução
- [x] Vigilância de desktop de entrada por `OpenInputDesktop` (200 ms)
- [x] Injeção por `SendInput` com `KEYEVENTF_SCANCODE`
- [x] `SendSAS` carregado de `sas.dll`, com a política `SoftwareSASGeneration`
- [x] Matriz de origem confiável — 4 configurações, escolhidas por `POC1_MODE`
- [x] Instalação em `%ProgramFiles%` com ACL de administrador, exigida pelo UIAccess
- [x] Instalador recusa build anterior ao endurecimento de janeiro de 2026
- [x] Desinstalação sem resíduo, preservando o registro do resultado
- [ ] `[H]` **Executar em bancada Windows** — ver `spikes/poc1-winlogon/README.md`
- [ ] `[H]` Item 1: sequência aparece no campo da tela de bloqueio
- [ ] `[H]` Item 2: máquina desbloqueia com a senha digitada remotamente
- [ ] `[H]` Item 3: funciona na tela de login pós-boot (nível N3)
- [ ] `[H]` Item 4: funciona em prompt de UAC
- [ ] `[H]` Item 5: troca de desktop detectada em < 300 ms
- [ ] `[H]` Item 6: `SendSAS` produz a tela de Ctrl+Alt+Del
- [ ] `[H]` **Nível de capacidade do Windows declarado** (N3 / N2 / N1)

### PoC-2 — Bluetooth RFCOMM dentro de um serviço
- [ ] Socket `AF_BTH` + `BTHPROTO_RFCOMM` no Windows, a partir de serviço
- [ ] Publicação de serviço SDP por `WSASetService`
- [ ] Backend BlueZ por `ProfileManager1.RegisterProfile`
- [ ] Medidor de RTT com carga de 125 msg/s
- [ ] `[H]` Socket abre na sessão 0, sem usuário logado
- [ ] `[H]` Par continua pareado após reiniciar as duas máquinas
- [ ] `[H]` Windows↔Windows e Windows↔Linux, nos dois sentidos
- [ ] `[H]` Latência: mediana < 20 ms, p99 < 50 ms
- [ ] `[H]` Reconexão < 5 s após religar o rádio
- [ ] `[H]` MTU efetiva medida nas duas pilhas
- [ ] `[H]` Comparação lado a lado com UDP cabeado e UDP Wi-Fi

### PoC-3 — `uinput` no greeter e na tela de bloqueio do Linux
- [ ] Criação dos três dispositivos virtuais
- [ ] Confirmação de enumeração por `libudev` antes de declarar pronto
- [ ] `[H]` Senha na tela de bloqueio do GNOME e do KDE (N2)
- [ ] `[H]` Senha no greeter do GDM (N3)
- [ ] `[H]` Atraso de enumeração medido
- [ ] `[H]` SELinux em *enforcing* no Fedora
- [ ] `[H]` Ponteiro absoluto acertando o pixel em telas de escalas diferentes
- [ ] `[H]` Funciona sem `root`, como usuário de sistema dedicado
- [ ] `[H]` **Nível de capacidade do Linux declarado** (N3 / N2 / N1)

### PoC-4 — `InputCapture` + `libei` como servidor
- [ ] Sessão de portal, `GetZones`, `SetPointerBarriers`, `ConnectToEIS`
- [ ] Persistência e reapresentação do `restore_token`
- [ ] `[H]` Barreiras nas quatro bordas, GNOME e KDE
- [ ] `[H]` Atraso mediano de evento < 5 ms
- [ ] `[H]` Segunda execução sem diálogo de permissão

### PoC-5 — Noise sobre os três portadores
- [ ] `Noise_XX` + código de seis dígitos derivado do handshake
- [ ] `Noise_IK` com chave estática fixada
- [ ] Janela deslizante de repetição (2 048 bits)
- [ ] Mesmo código sobre stream e datagrama
- [ ] Custo de cifrar/decifrar mensagem de entrada < 20 µs
- [ ] `[H]` Latência adicionada em UDP na LAN: mediana < 8 ms

### PoC-6 — Empacotamento
- [ ] Um comando gera instalador, ZIP, RPM e DEB
- [ ] Instalar e desinstalar sem resíduo

---

## Etapa 1 — Esqueleto

### 1.1. Fundação do repositório
- [x] `git init`, `.gitignore`, `.gitattributes`, `LICENSE` (MIT)
- [x] Workspace Cargo com `resolver = "3"`, edição 2024
- [x] Política de lints do workspace conforme [09](docs/09-padroes-de-codigo.md)
- [x] `rustfmt.toml` e `rust-toolchain.toml` fixando a versão
- [x] `PROGRESSO.md` e `LOG.md` (índice de `docs/logs/`)
- [x] `clippy.toml` com os limites verificáveis pelo clippy e os nomes próprios do projeto
- [x] `deny.toml` com licenças permitidas, avisos do RustSec e fontes confiáveis
- [x] CI: `fmt`, `clippy -D warnings`, `test`, `doc`, `xtask check`, `deny`, nos dois sistemas

### 1.2. `xtask` — as regras que o CI faz cumprir
- [x] `check-limits`: linhas por arquivo, por função e por crate
- [x] `check-deps`: setas de dependência de [02, §2](docs/02-arquitetura.md)
- [x] `check-deps`: pureza — crates puros sem runtime, relógio, E/S ou API de sistema
- [x] `check-logs`: nenhuma macro de log recebendo tipo de entrada
- [x] 23 testes do próprio `xtask` — uma verificação sem teste não é de confiança
- [ ] Parâmetros por função e aninhamento (delegados ao `clippy.toml`, a confirmar no CI)

### 1.3. Processos e IPC
- [x] `ir-ipc`: protocolo de controle daemon↔ui e daemon↔agente
- [x] Vocabulário próprio do contrato, para `ir-ui` não depender de `ir-proto`
- [x] Canal do agente separado do canal da interface, com vocabulários que não se misturam
- [x] Cada pedido declara a autoridade que exige, como propriedade do próprio pedido
- [ ] Transporte: named pipe no Windows com SDDL restrito
- [ ] Transporte: socket Unix `0660 root:inputremote`
- [~] Autorização em três níveis de [04, §5](docs/04-seguranca.md) — declarada no contrato;
      a imposição depende do transporte, que lê o token do cliente
- [ ] `ir-daemon`: binário sobe, aceita IPC, encerra limpo
- [ ] `ir-agent`: binário conecta, reporta pronto, encerra com o serviço
- [x] `ir-ui`: janela abre, mostra "sem par" e a impressão digital

### 1.4. Observabilidade e configuração
- [ ] `tracing` com escritor sem bloqueio
- [ ] Configuração `toml` com escrita atômica
- [ ] Caminhos de sistema por plataforma ([02, §7](docs/02-arquitetura.md))
- [ ] Relatório de diagnóstico por lista de campos permitidos

### 1.5. Instalação
- [x] Nomes de executável conforme [00](docs/00-indice.md): `inputremote-ui`, não `ir-ui`
- [x] Um comando gera os dois instaladores em `dist/`
- [x] Windows: instalador `.msi` (WiX), com atalho, entrada em Programas e desinstalação
- [x] Instalação em `%ProgramFiles%`, que é o requisito de pasta protegida do `UIAccess`
- [x] Assinatura Authenticode com certificado autoassinado, para teste e uso local
- [x] Manifesto com impressão digital, estado da assinatura e o que **falta** no pacote
- [x] Linux: RPM do Fedora 44, construído dentro do sistema de destino
- [~] Registro e remoção do serviço no Windows — declarado no MSI (`ServiceInstall`), mas não
      exercitado: `inputremote-daemon` ainda não existe
- [ ] Unidade `systemd` + regra `udev` + política D-Bus no Linux — entram no RPM junto com o
      serviço, que é quem os usa
- [ ] Assinatura com certificado de verdade e GPG no RPM ([Etapa 10](#etapa-10--qualidade-e-lançamento))

---

## Etapa 2 — Núcleo sem E/S

### 2.1. `ir-proto`
- [x] Tipos base: `HidUsage`, `Modifiers`, `Button`, `Buttons`, `Carrier`, `ChannelId`
- [x] `PressedKeys` e `InputState` limitados, com reconciliação idempotente
- [x] `Sequence` com aritmética de número de série (RFC 1982) e `Ack` com janela de 32
- [x] Arranjo de telas (`ScreenLayout`, `MonitorInfo`, `Edge`) validado
- [x] `MachineName`, `Capabilities` e os níveis `PrivilegedInputLevel` (N0–N3)
- [x] Catálogo de mensagens de [03, §6](docs/03-protocolo.md), um enum por canal
- [x] Codec `postcard` com o canal no primeiro byte, garantido por teste
- [x] Negociação de versão e recusa por incompatibilidade
- [x] Validação de manifesto e de caminho relativo contra travessia de diretório
- [x] Ida e volta de quadro em todos os portadores permitidos, e de todos os vetores
- [x] Teste de tamanho máximo (entrada ≤ 64 B em texto claro)
- [x] Vetores gravados da versão 1 — 16 quadros, 4 canais
- [x] Byte sobrando, truncamento em todo comprimento e lixo arbitrário não geram pânico
- [~] Alvo de `cargo fuzz` do decodificador — há varredura determinística no CI de commit;
      o alvo propriamente dito depende de `cargo-fuzz` e fecha junto com a Etapa 2
- [ ] Ida e volta de **toda** variante de `ClipboardMessage` e `BulkMessage`

### 2.2. `ir-geometry`
- [x] `Point` e `Rect` inteiros, sem ponto flutuante, com bordas inclusivas
- [x] `Desktop` a partir de `ScreenLayout`, com o principal como invariante estrutural
- [x] Monitor, retângulo, escala, arranjo, incluindo origens negativas
- [x] Mapeamento de coordenadas entre arranjos de resoluções diferentes, por fração
- [x] Normalização absoluta `0..=u16::MAX` com ida e volta exata até 65 536 px
- [x] Detecção de borda e ponto de entrada nas quatro direções
- [x] Recuo de um pixel na entrada, impedindo o ping-pong de travessia
- [x] Só a borda do par atravessa; as outras três prendem o ponteiro
- [x] Monitor removido durante a sessão não gera coordenada inválida
- [x] Buraco de arranjo em L tratado por `nearest_valid`
- [x] Deltas de `i32::MIN`/`i32::MAX` saturam em vez de estourar

### 2.3. `ir-session`
- [x] `Timestamp`/`Millis` injetados — o crate nunca lê o relógio
- [x] `Input` / `Command` / `CommandBatch` / `Session::step`
- [x] Máquina de estados: 4 fases, tabela de 10 transições, sem atalho para `Engaged`
- [x] Prazos coerentes por construção (`Timings::is_coherent`)
- [x] Handshake com negociação de versão e recusa por incompatibilidade
- [x] Política **única** de escolha de portador, com o motivo visível
- [x] Travessia de borda ida e volta, entre resoluções diferentes
- [x] `ReleaseAll` em toda falha, **antes** de qualquer outro comando
- [x] Reconexão sem novo pareamento
- [x] Troca de portador solta tudo antes de trocar
- [x] `StateSnapshot` periódico e reconciliação idempotente
- [x] Alinhamento de modificadores a cada mensagem de entrada
- [x] Atalho de emergência nos dois papéis
- [x] Coalescência de ponteiro, nunca de teclado
- [x] Latência de ida e volta medida e observável
- [x] Agente perdido devolve o controle sem deixar tecla presa
- [x] 25 cenários de integração, um por linha de [10, §2](docs/10-testes-e-validacao.md)
- [x] Confiabilidade sobre datagrama: janela, confirmação cumulativa com bitmap,
      retransmissão com `RTO = max(20 ms, 2 × srtt)`, e queda ao esgotar as tentativas
- [x] **Entrega em ordem** nos canais confiáveis — retransmissão cria fora de ordem, e um
      `KeyDown` chegando depois do `KeyUp` deixaria a tecla presa para sempre
- [x] Detecção de repetição: um datagrama reenviado não é aplicado duas vezes
- [x] Confirmação pura fora do fluxo ordenado, sem consumir janela nem sequência
- [x] Adeus anunciado em toda queda decidida por este lado
- [x] Um par emissor/receptor por canal, para retransmissão de clipboard não atrasar `KeyUp`
- [x] 29 testes da camada isolada + 6 cenários exercendo-a através da sessão inteira
- [ ] Cobertura ≥ 85% medida em `ir-session` e `ir-proto`

---

## Etapa 3 — Criptografia e pareamento
- [ ] `ir-crypto`: identidade estática X25519 persistente
- [ ] `Noise_XX` + código de seis dígitos (SAS)
- [ ] `Noise_IK` com chave fixada, recusa de chave diferente
- [ ] Janela de repetição e rechaveamento
- [ ] Armazenamento com ACL restrita e `zeroize`
- [ ] Teste de handshake adulterado e de repetição

## Etapa 4 — Rede
- [ ] `ir-net`: UDP de entrada com a confiabilidade do protocolo
- [ ] TCP de dados
- [ ] Descoberta mDNS + endereço manual
- [ ] Perda de 5% injetada não produz tecla presa
- [ ] Latência dentro da meta de [01, §6](docs/01-visao-e-escopo.md)

## Etapa 5 — Entrada no Windows
- [ ] Captura: Raw Input + `WH_*_LL`, gancho sem trabalho
- [ ] Supressão local e `ClipCursor`
- [ ] Injeção absoluta de ponteiro e por scancode
- [ ] Agente com thread por desktop ([ADR-0008](docs/adr/0008-agente-com-thread-por-desktop.md))
- [ ] Nenhum gancho no desktop `Winlogon`, verificado por teste
- [ ] `SendSAS` opcional na instalação
- [ ] `[H]` Nível de capacidade confirmado no produto (mínimo N2)
- [ ] `[H]` 10.000 travessias sem tecla presa

## Etapa 6 — Entrada no Linux
- [ ] Injeção por `uinput`, três dispositivos
- [ ] Captura por `InputCapture` + `libei`
- [ ] Integração com `logind`
- [ ] Filtro de auto-recaptura por dispositivo de origem
- [ ] `[H]` Quatro combinações entre plataformas
- [ ] `[H]` Nível de capacidade confirmado no produto (mínimo N2)

## Etapa 7 — Bluetooth
- [ ] `ir-bt`: trait + backend Winsock + backend BlueZ
- [ ] Política única de escolha de portador
- [ ] Reconexão
- [ ] `[H]` Quatro combinações por Bluetooth
- [ ] `[H]` Degradação para UDP com motivo visível

## Etapa 8 — Clipboard e arquivos
- [ ] `ir-clip`: texto, imagem PNG, lista de arquivos
- [ ] `ir-files`: manifesto, blocos, BLAKE3, cotas, staging por RAII
- [ ] Progresso e cancelamento
- [ ] Transferência de 5 GB degrada a entrada em no máximo 10%

## Etapa 9 — Interface

> **Fora de ordem, e de propósito.** A regra 5 diz que uma etapa com item aberto bloqueia a
> próxima, e a Etapa 1.3 ainda tem transporte pendente. A interface foi adiantada porque ela é o
> que revela se o contrato de `ir-ipc` serve — e revelou: três campos e um pedido faltavam
> ([log 08](docs/logs/08-ir-ipc.md)). Ela roda contra `ServicoSimulado`, e os itens que dependem
> do serviço de
> verdade continuam abertos ou em `[~]`.

- [x] Linguagem visual única em `ui/tema.slint`; tema claro e escuro seguindo o do sistema
- [x] Telas de estado, pareamento e preferências, com voltar explícito em vez de abas
- [x] Seletor visual de borda: duas telas desenhadas, no lugar de quatro botões de rádio
- [x] Uma ação em destaque por tela e, no máximo, um impedimento por vez
- [x] Estado observável de [01, §5](docs/01-visao-e-escopo.md) na tela inicial
- [x] Nível de capacidade visível sempre, e não escondido em preferências
- [x] Serviço simulado, para a interface rodar e ser testada antes de existir transporte
- [x] A janela avisa quando o serviço real não está respondendo
- [x] Diagnóstico em campo selecionável, por lista de campos permitidos
- [~] Fluxo de pareamento com código de seis dígitos — a interface está pronta e testada; o
      código de verdade depende da Etapa 3
- [ ] Preferências avançadas: arranjo de telas, atalho de emergência
- [ ] Bandeja do sistema
- [ ] Fechar, matar ou não abrir não altera a sessão

## Etapa 10 — Qualidade e lançamento
- [ ] Assinatura de todos os binários do Windows
- [ ] Instaladores e pacotes
- [ ] Documentação de usuário
- [ ] `[H]` Roteiro de validação física completo, quatro combinações
