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

**A janela sem ícone no Linux.** A janela nascia sem `app_id`: o GNOME não tinha como ligá-la ao
`inputremote.desktop`, e por isso ela aparecia sem ícone na barra e o lançador não a reconhecia como
já aberta. Agora a interface declara `slint::set_xdg_app_id("inputremote")` antes de a janela
existir — o mesmo nome do arquivo `.desktop`, que é o que o GNOME procura —, e o `StartupWMClass`
acompanha.

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
- as teclas que faltavam: `PrintScreen` (com e sem Alt) contra o asterisco do numérico, os dígitos
  do numérico contra o bloco de navegação, e a conferência dos **dois** backends contra
  `teclado_completo()`;
- a classe do dispositivo: computadores reais (notebook `0x1c010c`) entram; telefone, fone,
  alto-falante, teclado, mouse, relógio e classe ausente saem; o `Class=` do BlueZ lido;
- a guarda de dupla codificação.

**Bancada:** _a preencher com o resultado dos pacotes novos nas duas máquinas._

**Verificação:** _a preencher._
