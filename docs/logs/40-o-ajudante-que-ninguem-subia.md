# O ajudante que ninguém subia, os arquivos que não achavam o par, e as teclas que sumiam

**Data:** 2026-09-19 e 2026-09-20

**Itens:** Etapa 8 — copiar e colar de ponta a ponta; Etapa 4 — a lista de "Parear"; Etapa 3 —
teclado atravessando; Etapa 9 — a janela no Linux.

**O que foi feito:** Ctrl+C e Ctrl+V pararam de atravessar nos dois sentidos, pela segunda vez. Na
lista de "Parear" do Windows apareciam fones, alto-falantes e teclados ao lado do Fedora.

## As causas, pela bancada

1. **O ajudante de clipboard não estava rodando em nenhum dos dois.** No Windows havia o serviço, o
   agente e a janela — e nenhum `inputremote-agent --clipboard`. No Linux, a unidade gerada do
   autostart (`app-inputremote\x2dclipboard@autostart.service`) estava `inactive (dead)`: nunca
   subiu. O ajudante nascia **só no login** — chave `Run` no Windows, autostart do XDG no Linux. A
   sessão já estava aberta quando o pacote foi instalado, e a instalação do MSI encerrou o ajudante
   antigo sem subir o novo. É a mesma causa da primeira vez em que parou: cada atualização matava o
   ajudante, e só um novo login o trazia de volta. Sem ele o serviço não tem quem leia nem quem
   escreva o clipboard, e nada na tela dizia isso.
2. **Arquivos nunca achariam o par.** Os dois lados parearam pelo Bluetooth, e cada `config.toml`
   guardou só o endereço do rádio do outro (`AC:50:DE:47:EB:28`, `74:13:EA:A6:5A:99`). Arquivo vai
   só por TCP, e o canal de arquivos só discava endereço de rede: sem ele, esperava para sempre.
3. **A lista de pareados do sistema traz tudo** o que um dia foi pareado por Bluetooth.

## O que mudou

**O ajudante sempre de pé, por quem sempre está de pé.**

- Windows: o **serviço** lança o ajudante na sessão de console, com o token de quem entrou
  (`WTSQueryUserToken`, ambiente do usuário por `CreateEnvironmentBlock`), e o relança sempre que
  nenhum estiver ligado por 12 s (`ir-daemon/src/zelador.rs`). A chave `Run` do ajudante saiu do MSI.
- Linux: unidade do `systemd` do usuário, `inputremote-clipboard.service`, com `Restart=always` e
  sem limite de tentativas, ligada a `graphical-session.target` por um link que vem no pacote. O
  `%posttrans` a (re)inicia em toda sessão gráfica aberta (`/usr/libexec/inputremote/ajudante-nas-sessoes`),
  e a remoção a para. O autostart do XDG saiu.
- O ajudante se apresenta com `Pedido::AcompanharClipboard`, e o serviço conta quantos estão
  ligados enquanto a conexão vive: é a contagem que decide relançar, e o diagnóstico a mostra
  ("ajudantes de clipboard ligados").
- Trava de instância única por usuário (`File::try_lock`, solta pelo sistema quando o processo
  morre): relançar nunca produz dois ajudantes oferecendo a mesma cópia.

**Arquivos acham o par na rede.** O canal de arquivos recebe um `Localizador`: sem endereço de rede
configurado, ou quando o configurado não atende, ele pergunta à descoberta (`IRDESC`, log 39) onde
está a máquina cujo id é o da chave fixada — o id de máquina já é derivado dos 16 primeiros bytes da
chave pública. Ninguém respondendo, pergunta de novo em 15 s, para não encher a rede de broadcast.

**Só computadores na lista.** `ir_bt::Dispositivo` passa a carregar a classe do dispositivo
(`ulClassofDevice` no Windows, `Class=` do BlueZ no Linux), e a junção dos candidatos fica só com a
classe maior 0x01 (computador). Classe desconhecida conta como "não": a rede e o endereço digitado
continuam alcançando um computador que não a declare.

**O PrintScreen e mais vinte teclas que sumiam.** A tabela de scancodes do Windows ia de F12
(`0x45`) direto a Insert (`0x49`): faltavam `PrintScreen`, `Scroll Lock`, `Pause`, o **teclado
numérico inteiro** e a tecla de menu. O gancho recebia a tecla, não achava HID nenhum e a descartava
ali — apertar `PrintScreen` no servidor não fazia nada no cliente, sem erro e sem registro. Os
scancodes foram conferidos na própria API do Windows (`MapVirtualKeyW(…, MAPVK_VK_TO_VSC_EX)`), e
com eles duas armadilhas: o `PrintScreen` com Alt manda `0x54` (o SysRq) em vez de `E0 37`, e cada
tecla do bloco numérico divide o scancode com uma do bloco de navegação — o que as separa é o bit
estendido. O `Pause` fica de fora de propósito: no conjunto 1 ele é a sequência `E1 1D 45`.

E três teclas do **teclado brasileiro** não estavam em nenhum dos dois lados: a `\ |` à esquerda do
Z (`VK_OEM_102`, scancode `0x56`), a `/ ? °` à esquerda do Shift direito (`VK_ABNT_C1`, `0x73`) e a
vírgula do teclado numérico (`VK_ABNT_C2`, `0x7E`) — HID `0x64`, `0x87` e `0x85`, que no Linux são
`KEY_102ND`, `KEY_RO` e `KEY_KPCOMMA`.

A lista das teclas que o produto carrega virou contrato em `ir_proto::input::TECLADO_COMPLETO`, e os
**dois** backends são testados contra ela: uma tecla que só um lado saiba traduzir some naquele
sentido, calada. Junto, uma ferramenta de bancada — `cargo run -p ir-input --example teclas` — que
mostra o que o gancho capturou, para a pergunta "esta tecla atravessa?" não exigir as duas máquinas.

**O menu de contexto que aparecia a cada volta do ponteiro.** Voltando do cliente para o servidor,
o programa em foco no Windows abria um menu — o do botão direito, diferente em cada programa. A
causa é o "solta tudo", o comando mais importante do produto: ele era **literal**, e soltava toda
tecla e todo botão que o backend sabe emitir, tivessem sido apertados ou não. No Windows soltar o
que não está preso não é inócuo: o botão direito solto gera o menu de contexto do programa em foco,
e o Alt solto ativa a barra de menus. Agora cada injetor guarda o que apertou (`ir-input/pendentes`)
e solta só isso, nos dois sistemas — continuando idempotente, que era a razão de soltar tudo.

**Copiar e colar era mudo, e isso virava erro de percepção.** O relato: copiou a pasta `teste`,
colou no Linux, funcionou; copiou `ffmpeg`, colou, e veio `teste` **de novo**. A explicação é
simples e o defeito não é o clipboard: quando a cópia não atravessa, o clipboard do outro lado
continua com a **anterior** — e colar traz aquilo, sem nenhum sinal de que é um resto. Nada na tela
dizia que havia cópia em andamento, que ela terminou, ou por que não foi. A interface sequer
tratava `Aviso::Transferencia`.

Agora conta, em três lugares, com as mesmas palavras (`ir_ipc::transferencia`, onde as frases
ficam porque **dois** programas as mostram):

- **na janela**, um cartão com o que está indo, quanto falta, onde ficou o que chegou, ou o motivo
  de não ter ido. Ele **fica** depois de terminar: a pergunta "aquilo copiou mesmo?" vem depois,
  quando a pessoa já está no outro computador;
- **no Windows**, um aviso no canto da tela, que aparece com a janela fechada — quem copia está no
  Explorer. Ele não rouba o foco (o foco anterior é devolvido) e some sozinho seis segundos depois
  do fim;
- **no Linux**, a notificação do sistema (`notify-send`, pelo ajudante de clipboard, que é quem
  sempre está de pé na sessão): começo e fim, e nada por quadro de progresso.

**A tecla que ficava presa.** Segurar o Ctrl no Windows, atravessar e soltá-lo do outro lado
deixava o Ctrl preso **aqui**: a supressão come o "soltar", e o Windows continua achando que a
tecla está apertada — clicar passa a selecionar vários itens. Antes isso era mascarado por acidente
pelo "solta tudo" cego; ao corrigir o menu de contexto, o disfarce caiu e o defeito apareceu.
Agora, ao devolver o controle, o produto solta os modificadores que o **sistema** ainda julga
apertados — e só eles. Um Alt preso vai disfarçado com uma tecla sem função, senão soltá-lo
ativaria a barra de menus.

**O ajudante do Windows escrevia no vazio.** Ele não tem console: a saída padrão dele não ia a
lugar nenhum, e uma falha ficava invisível — a única pista era copiar e colar parar. Agora escreve
em `%LOCALAPPDATA%\InputRemote\logs`, um arquivo por dia.

**A janela sem ícone no Linux.** A janela nascia sem `app_id`: o GNOME não tinha como ligá-la ao
`inputremote.desktop`, e por isso ela aparecia sem ícone na barra e o lançador não a reconhecia como
já aberta. Agora a interface declara `slint::set_xdg_app_id("inputremote")`, com o mesmo nome do
arquivo `.desktop`, e o `StartupWMClass` acompanha.

A primeira tentativa **não funcionava**, e só o registro do próprio programa disse por quê: a
chamada estava no começo do `main`, e ali ainda não há plataforma gráfica escolhida — ela falhava
com *"no Slint platform was initialized"*, calada, porque ninguém lia a saída de erro de um programa
sem console. Agora acontece depois de a primeira janela existir e antes de ela aparecer, que é
quando o `app_id` é lido. Foi o mesmo arquivo de registro criado nesta rodada que a revelou.

**Guarda contra texto estragado.** Um arquivo inteiro do `ir-transporte` tinha ido para o
repositório em UTF-8 lido como Windows-1252 ("estÃ¡"). Compilava — comentário não quebra a build.
`cargo xtask check-texto` agora falha nisso.

## A prova

Testes novos:

- o canal de arquivos entre duas máquinas que só se conhecem pelo endereço do rádio, achando uma à
  outra por um localizador (a regressão exata da bancada);
- a escolha de onde discar: configurado primeiro, rede quando não há ou quando ele não atende;
- a descoberta devolvendo só a máquina procurada;
- o serviço contando o ajudante enquanto a conexão dele vive, e não contando a janela;
- a decisão de relançar: tolerância para o ajudante vivo reconectar, e a mesma folga para o
  lançado ligar;
- a trava de instância única;
- as frases de cada fase da cópia: o andamento com tamanho e porcentagem, o destino de quem
  recebe, o motivo de quem não atravessou, e o tamanho legível em cada faixa;
- a notificação que não se repete a cada quadro de progresso, e que separa começo de fim;
- os modificadores presos: só os apertados entram na lista, e o Alt vai disfarçado;
- o "solta tudo" soltando só o que foi injetado: nada quando nada foi apertado (o defeito do menu),
  o modificador que continua apertado depois de a letra sair, e a repetição do teclado não
  duplicando;
- as teclas que faltavam: `PrintScreen` (com e sem Alt) contra o asterisco do numérico, os dígitos
  do numérico contra o bloco de navegação, e a conferência dos **dois** backends contra
  `teclado_completo()`;
- a classe do dispositivo: computadores reais (notebook `0x1c010c`) entram; telefone, fone,
  alto-falante, teclado, mouse, relógio e classe ausente saem; o `Class=` do BlueZ lido;
- a guarda de dupla codificação.

**Bancada**, Windows (`SAMSUNG-MAXUEL`) e Fedora 44 (10.0.0.135), **pareados só pelo Bluetooth**,
com os pacotes novos dos dois lados:

| O que | Resultado |
|---|---|
| Ajudante depois de instalar, Windows | o serviço o lançou sozinho: `ajudante de clipboard lançado na sessão do usuário pid=44748` |
| Ajudante depois de instalar, Linux | subiu na sessão aberta, sem novo login, e o serviço registrou `ajudante de clipboard ligado` |
| Canal de arquivos sem endereço de rede | `par achado na rede local para o canal de arquivos achado=10.0.0.135:52525`, e `canal de arquivos estabelecido` 40 ms depois |
| Texto Windows → Linux | chegou e foi publicado (`o que chegou está no clipboard`), inclusive numa cópia que o usuário fez por conta própria |
| Texto Linux → Windows | `IR-L2W-13465` no clipboard do Windows |
| Arquivo Windows → Linux | SHA-256 idêntico em `/var/lib/inputremote/recebidos` |
| Arquivo Linux → Windows | SHA-256 idêntico em `%ProgramData%\InputRemoteecebidos` |

Do retorno de cópia, com os dois lados falando de verdade:

| O que | Resultado |
|---|---|
| Aviso no canto (Windows) | a janela de 320×92 aparece no canto inferior direito durante a cópia |
| Ele rouba o foco? | não: a janela em foco é a mesma antes e durante |
| Ele some? | sim, 12 s depois já não existe |
| Notificação (Linux) | no barramento do GNOME: `InputRemote` · "Recebendo do outro computador" · `notif2-915 · 0% de 30,0 MB`, e no fim "Chegou: é só colar" |
| Quantas por cópia | duas — começo e fim, e não uma por quadro de progresso |
| Tecla presa (Windows) | `cargo run -p ir-input --example travadas`: o Ctrl é deixado apertado de propósito, o controle é devolvido, e o sistema volta a dizer que ele está solto |

A bancada da tecla presa é um exemplo do próprio `ir-input`, e não um teste: ela mede o **estado do
Windows** (`GetAsyncKeyState`), que nenhum teste de unidade alcança. Ela devolve o teclado ao estado
em que o encontrou mesmo quando falha.

Duas armadilhas de bancada para quem for conferir de novo: janela desenhada por GPU **não aparece**
em foto de tela (nem por `BitBlt`, nem por `PrintWindow`) — sai branca ou preta, e o jeito de provar
que ela existe é enumerar as janelas do processo; e `gdbus monitor --dest` **não** mostra chamada de
método, só sinal — quem vê a notificação sair é `dbus-monitor --session
"interface='org.freedesktop.Notifications',member='Notify'"`.

Uma armadilha da bancada, de novo (já estava no [log 34](34-copiar-aqui-colar-la.md)): com o protetor de
tela do GNOME ativo, o compositor não entrega a seleção, e `wl-paste` fica pendurado até o prazo. O
que acorda a tela por SSH é
`busctl --user call org.gnome.ScreenSaver /org/gnome/ScreenSaver org.gnome.ScreenSaver SetActive b false`.

**Verificação:** 852 testes no Windows; `verificar.sh` verde no container do Fedora; clippy e
`cargo xtask check`, agora com a conferência de dupla codificação.
