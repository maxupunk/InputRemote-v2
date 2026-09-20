# O ajudante que ninguém subia, e os arquivos que não achavam o par

**Data:** 2026-09-19

**Itens:** Etapa 8 — copiar e colar de ponta a ponta; Etapa 4 — a lista de "Parear".

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
- a classe do dispositivo: computadores reais (notebook `0x1c010c`) entram; telefone, fone,
  alto-falante, teclado, mouse, relógio e classe ausente saem; o `Class=` do BlueZ lido;
- a guarda de dupla codificação.

**Bancada:** _a preencher com o resultado dos pacotes novos nas duas máquinas._

**Verificação:** _a preencher._
