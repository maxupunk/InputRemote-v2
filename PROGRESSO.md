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
- [~] Socket `AF_BTH` + `BTHPROTO_RFCOMM` no Windows — abre, vincula o canal, **aceita
      conexão de entrada sem registro SDP** e leva um pareamento inteiro pelo produto, com a
      janela mostrando `portador: Bluetooth, motivo: Preferido`; falta repetir a partir do
      serviço, na sessão 0 (logs 26 e 27)
- [x] ~~Publicação de serviço SDP por `WSASetService`~~ — canal fixo, sem SDP (ADR-0009)
- [x] ~~Backend BlueZ por `ProfileManager1.RegisterProfile`~~ — sockets `AF_BLUETOOTH` sem
      D-Bus, canal fixo (ADR-0009); log 26
- [x] Medidor de RTT com carga de 125 msg/s — `ir-bt/examples/bancada.rs`, com o `Endpoint`
      de produção. Emite sem esperar resposta e casa a volta com a ida pelo número de
      sequência; 600 sondas a 123/s, zero perdas (logs 26 e 28)
- [ ] `[H]` Socket abre na sessão 0, sem usuário logado — e é o mesmo pré-requisito da
      travessia de entrada no Windows: o canal do agente é `Acesso::Restrito` (só SYSTEM e
      administradores), então só o serviço instalado hospeda o agente. A instância paralela
      sem elevação serve para rádio, pareamento e sessão, mas não para captura (log 29)
- [ ] `[H]` Par continua pareado após reiniciar as duas máquinas
- [~] `[H]` Windows↔Windows e Windows↔Linux, nos dois sentidos — Linux→Windows pela bancada
      (log 26) e Windows→Linux pelo produto inteiro (log 27), com os seis dígitos batendo nas
      duas telas; faltam Windows↔Windows e Fedora→Windows pelo produto
- [ ] `[H]` Latência **adicionada**: mediana < 20 ms, p99 < 50 ms — a meta é de **uma
      travessia** (carimbo na captura de uma máquina contra a injeção na outra,
      `docs/01` §6); a ida e volta entra só para alinhar relógios. O medido é **proxy de
      transporte**: ida e volta de 51,21 ms na mediana e 136,16 ms no p99 sob carga de
      123/s, contra 49,84 e 90,02 ms sequencial. A carga **refutou** a suspeita de *sniff*
      — mediana igual, cauda pior, zero perdas, que é assinatura de enfileiramento e custo
      fixo por quadro. Falta a causa, a MTU efetiva, e medir o que a meta pede (log 28)
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
- [x] `Noise_XX` + código de seis dígitos derivado do handshake — já no produto, na Etapa 3
      ([log 13](docs/logs/13-pilha-completa-mouse-cruzando.md))
- [x] `Noise_IK` com chave estática fixada — já no produto, na Etapa 3
      ([log 13](docs/logs/13-pilha-completa-mouse-cruzando.md))
- [x] Janela deslizante de repetição (2 048 bits) — já no produto, na Etapa 3
      ([log 13](docs/logs/13-pilha-completa-mouse-cruzando.md))
- [ ] Mesmo código sobre stream e datagrama
- [ ] Custo de cifrar/decifrar mensagem de entrada < 20 µs
- [ ] `[H]` Latência adicionada em UDP na LAN: mediana < 8 ms

### PoC-6 — Empacotamento
- [~] Um comando gera instalador, ZIP, RPM e DEB — gera o MSI e o RPM; faltam o ZIP e o DEB
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
- [x] `check-limits`: linhas por arquivo, por função e por crate — módulos de teste em arquivo
      próprio (`#[cfg(test)] mod x;`) não contam como produção ([log 35](docs/logs/35-o-texto-pelo-canal-4.md))
- [x] `check-deps`: setas de dependência de [02, §2](docs/02-arquitetura.md)
- [x] `check-deps`: pureza — crates puros sem runtime, relógio, E/S ou API de sistema
- [x] `check-logs`: nenhuma macro de log recebendo tipo de entrada
- [x] 26 testes do próprio `xtask` — uma verificação sem teste não é de confiança
- [ ] Parâmetros por função e aninhamento (delegados ao `clippy.toml`, a confirmar no CI)

### 1.3. Processos e IPC
- [x] `ir-daemon`: binário sobe, gera identidade, escuta UDP, pareia e estabelece a sessão
- [x] `ir-ipc`: protocolo de controle daemon↔ui e daemon↔agente
- [x] Vocabulário próprio do contrato, para `ir-ui` não depender de `ir-proto`
- [x] Canal do agente separado do canal da interface, com vocabulários que não se misturam
- [x] Cada pedido declara a autoridade que exige, como propriedade do próprio pedido
- [x] Transporte do canal de controle: named pipe no Windows / socket Unix, enquadrado por
      `ir_ipc::codec`, com o cliente da interface (`ServicoReal`) e um teste de ida e volta
      ([log 14](docs/logs/14-servico-de-ponta-a-ponta.md))
- [x] Cada canal declara quem pode abri-lo: SDDL explícito no *pipe* do Windows; no Linux, o
      serviço confere a credencial de quem conecta (`SO_PEERCRED`) contra a filiação ao grupo
      `inputremote` lida **na hora**, então um `usermod` vale sem sair da sessão nem reiniciar. O de
      controle aceita o usuário interativo, o do agente só o serviço
      ([log 16](docs/logs/16-o-servico-trancou-a-propria-janela.md),
      [log 17](docs/logs/17-a-janela-que-volta-e-o-grupo-que-vale-na-hora.md))
- [x] A janela reconecta ao serviço sozinha: a ligação tem dono próprio (`conexao`), separado de
      quem abre o canal (`conector`), e a janela mostra o motivo e o que fazer enquanto espera.
      Provado de ponta a ponta com *named pipes* reais — serviço derrubado e ressubido, a mesma
      janela voltou sem ser reaberta — e por testes nos dois sistemas: o serviço sobe depois da
      janela, cai e volta, recusa por permissão e depois aceita
      ([log 17](docs/logs/17-a-janela-que-volta-e-o-grupo-que-vale-na-hora.md))
- [~] Autorização em três níveis de [04, §5](docs/04-seguranca.md) — declarada no contrato; a
      imposição depende de o transporte ler a elevação do token do cliente, ainda não feita
- [x] A troca de papel pela janela só é aceita onde o papel funciona: o serviço recusa com
      `Falha::PapelIndisponivel` sem gravar nada, e um servidor gravado numa plataforma sem
      captura sobe como cliente e tem o arquivo corrigido — verificado no Linux, onde a recusa
      existe ([log 18](docs/logs/18-a-prova-no-notebook-e-a-troca-de-papel.md),
      [log 19](docs/logs/19-a-troca-que-vale-na-hora.md))
- [x] Trocar papel ou borda vale na hora, em vez de ficar pendente até o serviço reiniciar: a
      sessão é recriada pelo mesmo caminho que solta tudo em toda queda. A borda tinha o mesmo
      defeito, pior — a janela mostrava a nova e a travessia usava a velha
      ([log 19](docs/logs/19-a-troca-que-vale-na-hora.md))
- [x] A borda é do servidor: só ele escolhe, e anuncia ao estabelecer e a cada troca
      (`EdgeConfig`); o cliente usa a oposta e grava, e recusa escolher (`Falha::BordaDoServidor`).
      Trocar a borda ajusta a sessão em uso em vez de refazê-la — refazer mandava um adeus que
      dizia ao par para não reconectar. Na bancada, as duas máquinas tinham terminado com `left`
      ([log 24](docs/logs/24-a-borda-e-do-servidor.md))
- [x] `ir-daemon`: binário sobe, aceita IPC, pareia pela interface, encerra limpo
- [x] `ir-agent`: binário conecta, reporta pronto, captura e injeta na sessão do usuário, e
      encerra com o serviço — exercitado de verdade: o serviço lança, o agente conecta, informa a
      tela da sessão e se reporta pronto ([log 15](docs/logs/15-agente-de-sessao.md)). No Windows,
      até `7c1aea0` o movimento capturado só saía quando o serviço mandava algum comando — o
      impasse do *pipe* síncrono ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))
- [x] Transporte do canal do agente, separado do da interface, e o serviço lançando o agente na
      sessão de console (`CreateProcessAsUserW` + `TokenUIAccess`)
- [x] `ir-ui`: janela abre, acha o serviço de verdade e pareia por ele; sem o serviço, diz o
      motivo e o que fazer, e entra sozinha quando ele sobe. O simulado só com `--simulado`. No
      Windows instalado, até `7c1aea0` ela travava ao abrir e ao pedir, e o pareamento nunca
      fechava. Com `7c1aea0` instalado ela responde em 0,28 s, e o pareamento com o notebook fechou
      ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))
- [x] Canal de quem conecta sem impasse no Windows: janela e agente abrem o *pipe* por
      `ir_ipc::cliente`, com E/S sobreposta, porque um *pipe* síncrono trava a escrita enquanto
      outra thread espera ler. Provado por teste contra *named pipe* real, com um segundo teste
      mostrando que o jeito antigo trava ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))
- [x] A janela aberta no meio de um pareamento recebe o código pendente ([log 39](docs/logs/39-parear-sem-configurar-nada.md))
- [ ] Uma leitura presa termina quando a janela descarta o canal com o serviço vivo — hoje a
      thread (e o runtime do cliente) ficam até o serviço fechar o *pipe* ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))

### 1.4. Observabilidade e configuração
- [x] `tracing` com escritor sem bloqueio — por fila (`tracing-appender`); o serviço do Windows
      registra em `%ProgramData%\InputRemote\logs`, um arquivo por dia, sete guardados; no Linux
      segue para o `journald` ([log 20](docs/logs/20-atualizar-sem-reiniciar-e-o-balanco.md))
- [x] Configuração `toml` com escrita atômica — arquivo temporário e `rename`, e a identidade
      gravada do mesmo jeito, com `0600` no Linux
      ([log 20](docs/logs/20-atualizar-sem-reiniciar-e-o-balanco.md))
- [~] Caminhos de sistema por plataforma ([02, §7](docs/02-arquitetura.md)) — o estado vai para
      `%ProgramData%\InputRemote` no serviço do Windows e para `/var/lib/inputremote` no Linux, mas
      não na divisão da especificação: a configuração do Linux deveria estar em `/etc/inputremote`,
      e a identidade e os pares do Windows numa subpasta `state\`. Mudar agora exige migrar
      instalações existentes ([log 20](docs/logs/20-atualizar-sem-reiniciar-e-o-balanco.md))
- [x] Relatório de diagnóstico por lista de campos permitidos — o do serviço é um formato fechado
      de campos, sem nada do que foi digitado
      ([log 20](docs/logs/20-atualizar-sem-reiniciar-e-o-balanco.md))

### 1.5. Instalação
- [x] Nomes de executável conforme [00](docs/00-indice.md): `inputremote-ui`, não `ir-ui`
- [x] Um comando gera os dois instaladores em `dist/`
- [x] Windows: instalador `.msi` (WiX), com atalho, entrada em Programas e desinstalação
- [x] Instalação em `%ProgramFiles%`, que é o requisito de pasta protegida do `UIAccess`
- [x] Assinatura Authenticode com certificado autoassinado, para teste e uso local
- [x] Manifesto com impressão digital, estado da assinatura e o que **falta** no pacote
- [x] Ícone no `.exe`, na entrada de Aplicativos e no tema `hicolor` do Linux
- [x] Linux: RPM do Fedora 44, construído dentro do sistema de destino — leva a interface **e** o
      serviço; conferir o conteúdo do pacote (e não só o fato de ele sair) foi o que revelou que
      até então ele levava só a interface ([log 15](docs/logs/15-agente-de-sessao.md))
- [x] Registro e remoção do serviço no Windows — `inputremote-daemon` responde ao SCM
      (`windows-service`), então o `StartService` do instalador conclui em vez de estourar o
      tempo; fora do SCM, o mesmo binário cai para primeiro plano
      ([log 14](docs/logs/14-servico-de-ponta-a-ponta.md))
- [~] Atualizar por cima sem reiniciar o Windows — o instalador desliga o Restart Manager, que
      via o agente (sem janela, como SYSTEM) em uso antes de o serviço parar e pedia reinício; e o
      serviço agora só se declara parado depois de soltar tudo e dispensar o agente. Compilado e
      testado; falta a pessoa instalar por cima e confirmar que não pede reinício
      ([log 20](docs/logs/20-atualizar-sem-reiniciar-e-o-balanco.md))
- [~] Unidade `systemd` no Linux — o RPM agora traz o serviço **e** a unidade, que roda como
      root (é quem tem `/dev/uinput`). Falta a regra `udev` e a política D-Bus, que só fazem
      sentido junto com o usuário dedicado do endurecimento
- [x] Do pacote ao primeiro uso sem terminal no Linux: o serviço é habilitado e iniciado na
      instalação e parado na remoção; a janela pede a senha pelo polkit, com a explicação, para
      liberar o acesso — e entra na hora ([log 38](docs/logs/38-a-senha-pedida-pela-janela.md))
- [ ] `[H]` O diálogo de senha do polkit, com a mão no teclado, no GNOME e no KDE ([log 38](docs/logs/38-a-senha-pedida-pela-janela.md))
- [x] O empacotador acha os binários em `CARGO_TARGET_DIR` quando ele está definido, em vez de
      empacotar em silêncio os de `target\release` ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))
- [x] Regra de firewall do serviço no instalador do Windows (sub-rede local, por programa) e serviço
      do firewalld no Linux ([log 39](docs/logs/39-parear-sem-configurar-nada.md))
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
- [x] Vetores gravados da versão 1 — 31 quadros, os **seis** canais, incluindo a época da sessão
      ([log 22](docs/logs/22-a-sessao-que-reiniciava-a-cada-200-ms.md),
      [log 30](docs/logs/30-o-canal-de-dados-em-tcp.md))
- [x] Byte sobrando, truncamento em todo comprimento e lixo arbitrário não geram pânico
- [~] Alvo de `cargo fuzz` do decodificador — há varredura determinística no CI de commit;
      o alvo propriamente dito depende de `cargo-fuzz` e fecha junto com a Etapa 2
- [x] Ida e volta de **toda** variante de `ClipboardMessage` e `BulkMessage` — 5 + 9 vetores
      gravados, com a contagem de variantes conferida por teste
      ([log 30](docs/logs/30-o-canal-de-dados-em-tcp.md))

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
- [x] Pico de latência vira atraso, e não queda: a espera entre reenvios dobra, e o enlace só
      cai quando uma mensagem passa de 1 s sem confirmação, contado do primeiro envio. Um
      travamento de 400 ms no meio do uso não derruba ninguém
      ([log 23](docs/logs/23-o-pico-de-latencia-que-virava-queda.md))
- [x] Encarnações de sessão: todo quadro carrega a época de quem o enviou, e o que é de uma
      sessão encerrada não ancora a seguinte, não a derruba e não vira tecla digitada. Os três
      cenários do laço de ~200 ms falhavam antes da correção e passam depois
      ([log 22](docs/logs/22-a-sessao-que-reiniciava-a-cada-200-ms.md))
- [ ] Cobertura ≥ 85% medida em `ir-session` e `ir-proto`

---

## Etapa 3 — Criptografia e pareamento
- [x] `ir-crypto`: identidade estática X25519 persistente (chave gravada, pública derivada)
- [x] `Noise_XX` + código de seis dígitos (SAS); teste de homem no meio produzindo códigos diferentes
- [x] `Noise_IK` com chave fixada, recusa de chave diferente (verificado no daemon)
- [x] Janela de repetição de 2048 bits (RFC 6479); `should_rekey` por volume
- [x] Armazenamento com `zeroize` e permissão 0600 na chave
- [x] Teste de handshake adulterado e de repetição
- [ ] Rechaveamento automático executado (hoje só sinalizado por `should_rekey`)

## Etapa 4 — Rede
- [x] `ir-net`: UDP de entrada cifrado, com o endpoint por canais
- [x] Pareamento de ponta a ponta testado (dois endpoints em loopback; código igual, confirmação dupla, quadro atravessa)
- [x] Descoberta na rede local e endereço manual — pergunta própria por broadcast (52524/UDP), no
      lugar do mDNS que o serviço do Windows (SYSTEM) não conseguia usar; a lista junta rede,
      Bluetooth pareado e o endereço da configuração, e a janela aceita o endereço digitado ([log 39](docs/logs/39-parear-sem-configurar-nada.md))
- [x] Um pedido de pareamento sem resposta termina em até 20 s com motivo (`Falha::ParNaoRespondeu`),
      e o handshake reenvia por até 12 s em vez de desistir no primeiro datagrama ([log 39](docs/logs/39-parear-sem-configurar-nada.md))
- [x] Quem recebe o pedido vai sozinho para os seis dígitos, até saindo da bandeja; o código é
      recontado à janela que abre depois ([log 39](docs/logs/39-parear-sem-configurar-nada.md))
- [x] Copiar e colar com retorno na tela: cartão na janela, aviso no canto (Windows) e notificação
      do sistema (Linux), com o que está indo, quanto falta, onde ficou e por que não atravessou
      ([log 40](docs/logs/40-o-ajudante-que-ninguem-subia.md))
- [x] O modificador que a supressão engolia deixava de ficar preso ao voltar o controle
      ([log 40](docs/logs/40-o-ajudante-que-ninguem-subia.md))
- [x] O agente e o ajudante de clipboard do Windows registram em arquivo: sem console, uma falha
      deles era invisível ([log 40](docs/logs/40-o-ajudante-que-ninguem-subia.md))
- [x] "Solta tudo" solta só o que o injetor apertou: soltar o botão direito que ninguém apertou
      abria o menu de contexto do programa em foco no Windows a cada volta do ponteiro
      ([log 40](docs/logs/40-o-ajudante-que-ninguem-subia.md))
- [x] Teclado inteiro atravessando do Windows: `PrintScreen`, `Scroll Lock`, o teclado numérico e a
      tecla de menu entraram na tabela de scancodes, e os dois backends são testados contra
      `ir_proto::input::teclado_completo()` — inclusive as três teclas a mais do ABNT2 brasileiro,
      que faltavam nos dois lados. Falta o `Pause`, que é a sequência `E1 1D 45`
      ([log 40](docs/logs/40-o-ajudante-que-ninguem-subia.md))
- [x] A janela do Linux declara `app_id`, e o ambiente gráfico a liga ao `.desktop`: ícone na barra
      e o lançador reconhecendo a janela aberta ([log 40](docs/logs/40-o-ajudante-que-ninguem-subia.md))
- [x] A lista de "Parear" mostra do Bluetooth só computadores (classe maior 0x01 da *Class of
      Device*); fones, alto-falantes e teclados pareados no sistema ficam de fora ([log 40](docs/logs/40-o-ajudante-que-ninguem-subia.md))
- [ ] O vencimento do código de pareamento é registrado como vencimento, e não com
      `reason="códigos diferentes"`, que aponta para alguém no meio ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))
- [x] Investigar `o handshake seguro falhou` registrado no cliente durante uma rediscagem de
      pareamento ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md)) — era o próprio serviço
      discando para parear a cada 3 s, por cima do handshake em andamento
      ([log 25](docs/logs/25-o-pareamento-que-se-desfazia-depois-do-clique.md))
- [x] Parear só começa pela janela; o pareamento dura do código na tela até o fim, sem rediscagem
      por cima e com a janela avisada se não terminar; esquecer o par encerra sessão e enlace. Na
      bancada, "São iguais" chegava ao serviço e o handshake se desfazia 1–3 s depois
      ([log 25](docs/logs/25-o-pareamento-que-se-desfazia-depois-do-clique.md))
- [ ] O handshake de pareamento não é abandonado pela rede antes do prazo do pareamento — hoje cai
      em ~105–110 s, contra os 120 s do ator e os "2 minutos" da tela ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))
- [ ] `[H]` A sessão firma e se mantém sobre Wi-Fi com economia de energia — na bancada ela caía em
      `Timeout` e se reiniciava a cada ~200 ms depois do pareamento ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md)).
      As duas causas estão corrigidas e testadas: quadros de uma sessão encerrada ancorando a
      seguinte ([log 22](docs/logs/22-a-sessao-que-reiniciava-a-cada-200-ms.md)), e a desistência em
      ~100 ms que transformava pico de latência em queda
      ([log 23](docs/logs/23-o-pico-de-latencia-que-virava-queda.md)). Instalado `c76242c` nas duas:
      90 s com a economia de energia ligada e picos de 117 ms, sem nenhuma queda, e a primeira ida e
      volta do controle no hardware. Mas o notebook trocou de ponto de acesso quatro vezes em dois
      minutos, e com um deles a sessão não firmou ou oscilou ([log 24](docs/logs/24-a-borda-e-do-servidor.md))
- [x] Reconexão por UDP depois de uma queda: o enlace aceita o reinício do par com a mesma chave,
      o iniciador ignora dado do enlace anterior, e uma regra de turno impede os dois lados de
      discarem juntos para sempre. Na bancada, a sessão passou a firmar de primeira ([log 35](docs/logs/35-o-texto-pelo-canal-4.md))
- [x] A confirmação alcança a mais antiga pendente: nada sai mais de 32 sequências à frente dela
      (`within_ack_reach`), senão o enlace caía por uma mensagem que chegou ([log 35](docs/logs/35-o-texto-pelo-canal-4.md))
- [ ] A sessão sobrevive à troca de ponto de acesso do Wi-Fi, com silêncios de mais de dez segundos
      no caminho — hoje cai pelo prazo de 1 s. Falta decidir entre prazo de queda maior e soltar
      tudo em 1 s mantendo a sessão ([log 24](docs/logs/24-a-borda-e-do-servidor.md))
- [~] Confiabilidade sobre UDP — a de `ir-session` já existe e é testada; falta o ensaio de perda de 5% ponta a ponta
- [x] TCP de dados — `ir-net::bulk`, ligado ao serviço pelo `ir-transferencia`. Provado entre
      duas instâncias: canal de pé em 8 ms, árvore de 8 itens atravessando nos dois sentidos com
      SHA-256 idêntico ([log 32](docs/logs/32-arquivos-atravessando.md))
      ([log 30](docs/logs/30-o-canal-de-dados-em-tcp.md), [ADR-0010](docs/adr/0010-canal-de-dados-em-tcp-proprio.md))
- [ ] `[H]` Latência dentro da meta, medida entre duas máquinas

## Etapa 5 — Entrada no Windows
- [x] Captura: `WH_MOUSE_LL` + `WH_KEYBOARD_LL`, gancho sem trabalho (fila e retorno)
- [x] Supressão local e prisão do ponteiro por `SetCursorPos`
- [x] Injeção absoluta de ponteiro e por scancode (`SendInput`)
- [x] Captura exercitada de verdade na sessão desbloqueada (477 eventos, deltas corretos)
- [~] Raw Input para deltas de alta resolução — hoje os deltas vêm do gancho; refinamento posterior
- [x] Agente de sessão: o serviço o lança na sessão de console e ele captura e injeta lá, que é
      o que faz o serviço instalado alcançar a área de trabalho do usuário (N1)
      ([log 15](docs/logs/15-agente-de-sessao.md))
- [ ] Agente com thread por desktop ([ADR-0008](docs/adr/0008-agente-com-thread-por-desktop.md)) — é o que leva de N1 a N2/N3
- [ ] Nenhum gancho no desktop `Winlogon`, verificado por teste
- [ ] `SendSAS` opcional na instalação
- [ ] `[H]` Nível de capacidade confirmado no produto (mínimo N2)
- [ ] `[H]` 10.000 travessias sem tecla presa

## Etapa 6 — Entrada no Linux
- [~] Injeção por `uinput` — no notebook de teste com GNOME, os dispositivos `InputRemote Keyboard`
      e `InputRemote Pointer` já aparecem registrados (a especificação fala em três; aparecem dois).
      Falta ver ponteiro e teclado reagirem de verdade, o que só acontece com o par conectado e a
      travessia feita ([log 20](docs/logs/20-atualizar-sem-reiniciar-e-o-balanco.md))
- [ ] Captura por `InputCapture` + `libei`
- [ ] Integração com `logind`
- [ ] Filtro de auto-recaptura por dispositivo de origem
- [ ] `[H]` Quatro combinações entre plataformas
- [ ] `[H]` Nível de capacidade confirmado no produto (mínimo N2)

## Etapa 7 — Bluetooth
- [x] `ir-bt`: trait + backend Winsock + backend BlueZ — log 26
- [x] Política única de escolha de portador — o serviço passou a rotear pelo portador que a
      sessão escolhe, em vez de mandar tudo pela rede; `ir-transporte` extraído. Log 26
- [x] Reconexão — com a chave fixada e o `peer_addr` de cada lado apontando para o rádio do
      outro, as duas máquinas reconectam sozinhas por Bluetooth ao subir, sem código e sem
      ninguém pedir (log 29)
- [~] `[H]` Quatro combinações por Bluetooth — duas feitas: Linux liga e Windows atende
      (log 26); Windows liga e Linux atende, pelo produto inteiro (log 27)
- [ ] `[H]` Degradação para UDP com motivo visível

## Etapa 8 — Clipboard e arquivos
- [x] Transporte do canal 5: `ir-net::bulk` — `u32` + corpo, `IK` sem pareamento, contador
      implícito, e a regra de colisão quando as duas pontas discam
      ([log 30](docs/logs/30-o-canal-de-dados-em-tcp.md))
- [~] `ir-clip`: texto e lista de arquivos, com a guarda de eco. Os dois backends exercitados
      contra clipboard de verdade: Windows (`AddClipboardFormatListener`, `CF_HDROP`) e Linux
      (`wl-clipboard`; no GNOME sem vigia, lido na travessia). Imagem PNG falta
      ([log 33](docs/logs/33-o-clipboard-sem-interceptar-atalho.md),
      [log 34](docs/logs/34-copiar-aqui-colar-la.md))
- [x] `ir-files`: manifesto, blocos, BLAKE3, cotas, staging por RAII — 69 testes, incluindo a
      travessia de uma árvore inteira e treze casos de par hostil
      ([log 31](docs/logs/31-o-motor-de-transferencia.md))
- [~] Progresso e cancelamento — o motor conta, o serviço anuncia por `Aviso::Transferencia` e a
      ferramenta de bancada mostra a barra; falta a tela do Slint
      ([log 32](docs/logs/32-arquivos-atravessando.md))
      ([log 31](docs/logs/31-o-motor-de-transferencia.md))
- [ ] `[H]` Transferência de 5 GB degrada a entrada em no máximo 10% — exige as duas máquinas
- [x] Ligar `ir-files` ao `ir-net::bulk` no serviço — `ir-transferencia`, na tarefa dele, fora do
      compasso de 5 ms da entrada ([log 32](docs/logs/32-arquivos-atravessando.md))
- [x] O canal de arquivos acompanha o par: parear e esquecer valem na hora, sem reiniciar o
      serviço; sem par, cada pedido é recusado com o motivo ([log 35](docs/logs/35-o-texto-pelo-canal-4.md))
- [x] `ir-clip` ligado ao produto — pelo ajudante `inputremote-agent --clipboard`, que roda como
      o usuário e fala pelo canal de controle, e não pelo agente, que é SYSTEM e carrega injeção
      ([ADR-0011](docs/adr/0011-clipboard-na-travessia.md), [log 34](docs/logs/34-copiar-aqui-colar-la.md))
- [x] O serviço só envia o que quem pediu poderia ler (`ir_files::permissao`), conferido no
      descritor aberto: `/etc/shadow` no clipboard foi recusado na bancada
      ([log 34](docs/logs/34-copiar-aqui-colar-la.md))
- [~] `[H]` Ctrl+C e Ctrl+V de arquivos de ponta a ponta — nos dois sentidos pela rede, com
      SHA-256 idêntico e o destino no clipboard do outro lado, com o serviço instalado dos dois
      lados ([log 36](docs/logs/36-o-servico-instalado-como-origem.md)); falta o Ctrl+V à mão no Nautilus
      ([log 34](docs/logs/34-copiar-aqui-colar-la.md))
- [x] Texto atravessando pelo canal 4 da sessão, em qualquer portador, até 256 KiB, conferido por
      BLAKE3: nos dois sentidos na bancada, e 218 KB em 2,8 s sem queda de sessão ([log 35](docs/logs/35-o-texto-pelo-canal-4.md))
- [x] O serviço do Windows (SYSTEM) sabe quem pediu o envio: token do cliente do *pipe* em nível de
      identificação e `AccessCheck` no arquivo já aberto (`ir-acesso`). Com o serviço instalado, uma
      pasta saiu e um arquivo que só o SYSTEM lê foi recusado ([log 36](docs/logs/36-o-servico-instalado-como-origem.md))
- [ ] Uma cópia recusada é oferecida duas vezes no Windows (dois avisos de mudança) ([log 36](docs/logs/36-o-servico-instalado-como-origem.md))
- [x] O ajudante de clipboard sempre de pé: o serviço do Windows o lança na sessão do usuário e o
      relança quando falta; no Linux, unidade do `systemd` do usuário com `Restart=always`, que o
      pacote (re)inicia nas sessões abertas. Instalar ou atualizar não deixa mais copiar e colar
      parado até o próximo login ([log 40](docs/logs/40-o-ajudante-que-ninguem-subia.md))
- [x] Arquivos entre pares pareados pelo Bluetooth: o canal de arquivos acha o par na rede local
      pela descoberta, pelo id de máquina da chave fixada ([log 40](docs/logs/40-o-ajudante-que-ninguem-subia.md))
- [ ] `[H]` Com troca rápida de usuário no Windows, o texto que chega vai aos ajudantes das duas
      sessões ([log 35](docs/logs/35-o-texto-pelo-canal-4.md))

## Etapa 9 — Interface

> **Fora de ordem, e de propósito.** A interface foi adiantada porque ela é o que revela se o
> contrato de `ir-ipc` serve — e revelou: três campos e um pedido faltavam
> ([log 08](docs/logs/08-ir-ipc.md)). Ela nasceu contra o `ServicoSimulado` e hoje fala com o
> serviço de verdade pelo canal de controle ([log 14](docs/logs/14-servico-de-ponta-a-ponta.md)).
> Sem o serviço, ela diz o motivo e reconecta sozinha; o simulado só entra com `--simulado`
> ([log 17](docs/logs/17-a-janela-que-volta-e-o-grupo-que-vale-na-hora.md)).

- [x] Linguagem visual única em `ui/tema.slint`; tema claro e escuro seguindo o do sistema
- [x] Telas de estado, pareamento e preferências, com voltar explícito em vez de abas
- [x] Seletor visual de borda: duas telas desenhadas, no lugar de quatro botões de rádio — só no
      computador que tem o teclado e o mouse ([log 24](docs/logs/24-a-borda-e-do-servidor.md))
- [x] Uma ação em destaque por tela e, no máximo, um impedimento por vez
- [x] Estado observável de [01, §5](docs/01-visao-e-escopo.md) na tela inicial
- [x] Nível de capacidade visível sempre, e não escondido em preferências
- [x] Serviço simulado, para a interface rodar e ser testada antes de existir transporte
- [x] A janela avisa quando o serviço real não está respondendo
- [x] Diagnóstico em campo selecionável, por lista de campos permitidos
- [x] Ícone do programa, desenhado para ler em 16 px, nos quatro lugares que o mostram
- [x] Nenhuma janela de console atrás da interface no build de release
- [x] Fluxo de pareamento com código de seis dígitos — ligado ao serviço de verdade: a janela
      mostra o código que o pareamento cifrado gera e a confirmação nas duas telas fecha o par
      ([log 14](docs/logs/14-servico-de-ponta-a-ponta.md)). No Windows instalado não fechava até
      `7c1aea0`: o clique ficava preso no *pipe* síncrono ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))
- [x] Informar o endereço do outro computador pela janela, sem editar arquivo como administrador
      ([log 39](docs/logs/39-parear-sem-configurar-nada.md))
- [ ] "Parear" com um pareamento automático já em curso não reinicia o *handshake* nem troca o
      código das duas telas ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))
- [ ] Botões acessíveis: `accessible-role` e ação padrão, para leitor de tela e automação
      ([log 21](docs/logs/21-a-janela-que-travava-no-windows.md))
- [ ] Preferências avançadas: arranjo de telas, atalho de emergência
- [~] Bandeja do sistema no Windows — ícone, menu, minimizar e fechar escondem, uma interface por
      sessão, sobe com o login já na bandeja; falta o clique de verdade no ícone ([log 37](docs/logs/37-a-bandeja-e-a-rolagem-de-lado.md))
- [x] Preferências sem rolagem horizontal: a área rolável tem a largura visível, e nenhum texto
      empurra a largura ([log 37](docs/logs/37-a-bandeja-e-a-rolagem-de-lado.md))
- [x] Ícone do atalho no menu Iniciar — o atalho anunciado do MSI não tinha ícone ([log 37](docs/logs/37-a-bandeja-e-a-rolagem-de-lado.md))
- [ ] Fechar, matar ou não abrir não altera a sessão

## Etapa 10 — Qualidade e lançamento
- [ ] Assinatura de todos os binários do Windows
- [ ] Instaladores e pacotes
- [ ] Documentação de usuário
- [ ] `[H]` Roteiro de validação física completo, quatro combinações
