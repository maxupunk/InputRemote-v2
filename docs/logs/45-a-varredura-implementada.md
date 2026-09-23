# A varredura implementada: o caminho de arquivo que saía da pasta, a chave que todos liam, e a tela de bloqueio que faltava

**Data:** 2026-09-23

**Itens:** todas as etapas — é a implementação do relatório da varredura de melhorias de 2026-09-22,
nas cinco frentes dele: segurança, tecla presa e estabilidade, experiência, arquitetura e
funcionalidades grandes.

**O que foi feito:** a varredura leu o código inteiro atrás de defeitos e de ausências. Duas falhas
de segurança graves foram confirmadas no código e nesta máquina antes de qualquer outra coisa. O
resto veio na ordem do relatório. Nada disto foi provado no hardware da bancada ainda — a lista do
que falta provar está no fim.

## 1. Segurança

**Um par gravava arquivo fora da pasta de recebidos, como SYSTEM.** A validação do caminho de um
item recebido só recusava `:` no começo. Um item `x/C:payload.dll` passava, e `PathBuf::push` o
transformava em `C:payload.dll` — fora da pasta, gravado pelo serviço. Agora cada componente é
conferido (`is_safe_component`: sem `:`, sem `\`, sem controle, sem nome reservado do Windows, sem
`.`/`..`), e a pasta de preparação confere de novo que o destino final começa pela raiz. Os nomes
de dispositivo (`CON`, `COM1`, `LPT9`…) entram em qualquer extensão.

**A chave privada da máquina era legível por qualquer usuário do Windows.** `icacls` no
`identity.key` instalado mostrava `BUILTIN\Usuários:(RX)`: `restrict()` não fazia nada no Windows, e
`%ProgramData%` dá leitura a todos. A pasta de estado recebe agora uma DACL **protegida**, só com
SYSTEM e Administradores, reaplicada a cada subida (`ir-acesso::seguranca::proteger_pasta`); a de
recebidos acrescenta o usuário interativo. No Linux, os arquivos privados nascem `0600` por
`create_new` + modo — o `chmod` depois deixava uma janela —, e falhar em restringir virou erro.

**Qualquer um na rede local travava a reconexão.** Um pedido de pareamento falso a cada poucos
segundos ocupava o transporte esperando uma confirmação que nunca vinha, e fazia aparecer um código
que ninguém pediu. Com par gravado, o pareamento de fora só é atendido nos três minutos depois de
alguém abrir o pareamento aqui (`actor/abertura.rs`); o transporte recusa antes de qualquer
criptografia, na rede e no Bluetooth (`AcceptPairing`). Teste: `ir-net/tests/pareamento_fechado.rs`.

**O envio confiava no caminho que o clipboard dava.** Um caminho UNC fazia o SYSTEM se autenticar
num servidor de fora; no Linux, trocar o arquivo por um FIFO travava o serviço. Caminhos relativos e
UNC são recusados (`caminho_local`), e o arquivo é aberto com `O_NONBLOCK` e conferido como arquivo
comum antes de ler.

Menores: o registro deixou de ter caminhos acima de `debug` e o código de pareamento; o pedido de
desligar a economia do Wi-Fi é atendido no máximo uma vez por minuto; o `powercfg`/`iw` tem prazo de
5 s; uma escrita no rádio tem prazo de 2 s, e não segura mais o endpoint inteiro.

## 2. Tecla presa e estabilidade

- **A sessão nunca sabia que o agente caiu.** O serviço passa `AgentReady`/`AgentLost` à sessão, que
  solta o par quando é servidor, e o agente é ressubido na hora.
- **A fila para o agente descartava `KeyUp` e `SoltarTudo` quando enchia**, com um `warn`. Um
  atraso do agente agora derruba a conexão — e o agente, ao perder o serviço, solta tudo.
- **O agente tem um vigia de supressão**: sem renovação do serviço a cada segundo, ele devolve o
  teclado e o mouse em 3 s. Uma tela de desktop que muda (Ctrl+Alt+Del, UAC) e o arranjo de monitores
  são avisados ao serviço.
- **O serviço do Windows se recupera sozinho**: o MSI configura reinício após falha, a subida que
  falha sai com código diferente de zero, e suspensão, retomada e bloqueio de sessão chegam ao ator.
  No Linux, o gancho de suspensão do `systemd` avisa por `SIGUSR1`/`SIGUSR2`.
- **O Bluetooth volta sozinho** se o adaptador sumir ou não existir na subida (`Reabridor`: 3 s,
  depois 15 s, para sempre).
- **O canal de arquivos sabe que o par sumiu**: prazo de conexão de 5 s, de handshake de 10 s,
  *keepalive* do TCP, e a porta ocupada é tentada de novo a cada 5 s em vez de recusar tudo até
  reiniciar (`ir-transferencia/tests/porta_ocupada.rs`).
- **As gravações da configuração saíram do laço do serviço** (`actor/gravador.rs`): o endereço do
  rádio, o nome do par e a borda chegam com a sessão de pé, e o `sync_all` delas era um tranco no
  ponteiro. A fila é única e em ordem, e as ações da janela ainda esperam o resultado.

## 3. Experiência

A janela recebia dados fixos: `latencia: None`, `ultima_queda: None`, nome "computador pareado". O
serviço agora mede o atraso (`Voltas`, com gráfico do último minuto), conta o motivo da queda e
guarda o nome que o par dá a si mesmo.

- **O cliente nunca ficava verde**: o aviso da tela de bloqueio aparecia sempre. Agora aparece
  quando é verdade — o par está na tela de bloqueio e recusa digitação.
- **O pareamento que ficava preso** em "Chamando o outro computador…" quando o serviço recusava volta
  com a explicação.
- **"Encerrar conexão" não encerrava**: virou **Pausar/Retomar**, com a conexão de pé e o par
  sabendo que é pausa (`MotivoDaQueda::ParPausou`), também pela bandeja.
- **Ações destrutivas pedem confirmação** (`BotaoConfirmado`): esquecer o par, limpar os recebidos,
  trocar o papel.
- **Uma frase só cobria falhas diferentes**: `Falha` ganhou `SemBluetooth`, `SemConexao`,
  `ParDesatualizado`, `EnderecoInvalido` e `SistemaRecusou`, cada uma com o que fazer.
- **A bandeja** mostra o estado na dica, e a queda vira notificação.
- **Acessibilidade**: os botões recebem foco e têm papel e ação padrão; o azul do tema escuro passou
  no contraste.
- **O cartão de cópia** tem **Cancelar** e **Abrir pasta**; o Windows ganhou **Iniciar o serviço**
  pela faixa do topo; o [USAR.md](../../USAR.md) foi reescrito — ainda descrevia o terminal.

## 4. Arquitetura

- **"Onde está o par" era calculado em quatro lugares**: `Alcance` é a fonte única, e alimenta
  também o destino do canal de arquivos. O pareamento pendente foi para dentro de `Pareamento`.
- **`ir-session` estava a 45 linhas do teto**: o protocolo de temporizadores, que o serviço nunca
  usava, saiu; as sequências foram para `ir-confiabilidade`.
- **O `ir-daemon` passava do teto** com o trabalho todo, e três fronteiras que já existiam viraram
  crates: `ir-servico` (o ciclo de vida do processo — SCM, sinais, `logind`, registro), `ir-painel`
  (a tradução do que o serviço sabe para o que a janela desenha, sem E/S) e `ir-area` (o canal de
  clipboard de texto, puro, com testes próprios).
- `apply_commands` clonava todos os comandos a cada volta; agora esvazia o lote.

## 5. Funcionalidades grandes

- **Tela de bloqueio, UAC e Ctrl+Alt+Del (R1)**: o agente injeta por uma thread por desktop
  (`Default`, `Winlogon`, protetor de tela), e o Ctrl+Alt+Del vai por `SendSAS` a partir do serviço,
  com a política `SoftwareSASGeneration` ajustada só quando o administrador permite. Protocolo 4:
  `SecureAttention`, `ProtectedDesktop{refused}`, `LockScreen`.
- **Atalhos** (`ir-session/src/session/secure.rs`): Ctrl+Alt+End (Ctrl+Alt+Del do outro),
  Ctrl+Alt+Shift+Esc (emergência: devolve e solta tudo), Ctrl+Alt+Shift+Espaço (troca de máquina).
- **Linux como o lado que controla**, por `evdev` + `EVIOCGRAB` ([06, §3.4](../06-linux.md)).
- **Vários monitores**: o arranjo vem de `EnumDisplayMonitors`, e o desktop virtual inteiro é a tela.
- **Travar a borda, bloquear juntos, gráfico de atraso** — as ideias das outras ferramentas.
- **Troca de chaves** a cada 2^20 quadros ou 10 minutos, na rede, sem derrubar a sessão
  ([03, §3.1](../03-protocolo.md)).
- **Imagem no clipboard**: PNG no protocolo; no Windows, o formato `PNG` quando existe e o DIB
  convertido quando não (`ir-clip/src/imagem.rs`, sem `unsafe`), e ao publicar os dois; no Linux,
  `image/png`. A imagem atravessa como um arquivo PNG de nome reconhecível pelo canal de dados, e o
  ajudante do outro lado a publica como imagem e apaga o arquivo.

### As metas medidas

| Meta | Medida | Onde |
|---|---|---|
| ponteiro a 125 Hz sem acumular | 10 000 travessias, nenhuma tecla presa | `ir-session/tests/metas.rs` |
| atraso da rede, loopback | mediana 0,28 ms | `ir-net/tests/latencia.rs` |

## A imagem no clipboard, nesta máquina

O único item provado fora dos testes: com o clipboard de verdade do Windows 11 daqui,

- o backend publicou um PNG de 2×1 e leu de volta os mesmos bytes;
- o WinForms (`Clipboard.GetImage()`), outro programa, viu **2×1** com vermelho e azul nos pixels
  certos — o `CF_DIB` que publicamos é lido por quem não conhece PNG;
- uma imagem posta pelo WinForms (só DIB) foi lida como PNG válido.

Os dois testes que fazem isso são `#[ignore]`, porque trocam o clipboard de quem roda.

## Os números

| | |
|---|---|
| Testes no Windows | 1 056 passam, 4 ignorados (os que mexem no clipboard e no hardware) |
| Testes no Linux (container do Fedora 44, sem a janela) | 985 passam |
| `clippy --workspace --all-targets` | limpo nos dois |
| `xtask` | 309 arquivos, tudo dentro das regras |

## O que ainda não foi provado em hardware

- a entrada na tela de bloqueio, no UAC e o Ctrl+Alt+Del com o serviço **instalado** e o
  certificado confiado — é o requisito central, e depende do MSI novo instalado como administrador;
- o Linux controlando o Windows numa sessão GNOME;
- imagem entre as duas máquinas (o Wayland e a travessia);
- suspender e acordar as duas máquinas com a conexão de pé; o Bluetooth voltando depois de o
  adaptador sumir;
- a troca de chaves por idade entre duas máquinas (o teste é em loopback).

## O que ficou de fora, de propósito

- **Troca de chaves no meio de um enlace Bluetooth**: as chaves se renovam a cada reconexão do
  RFCOMM. Um handshake dentro do fluxo pediria enquadramento próprio, e o contador de 64 bits do
  ChaCha20-Poly1305 não chega perto de se esgotar numa sessão ([03, §3.1](../03-protocolo.md)).
- **O portal `InputCapture`** no Linux: a captura por `evdev` resolve o papel de servidor hoje, com a
  travessia pela borda aproximada; o portal é o caminho exato e continua na Etapa 6.
