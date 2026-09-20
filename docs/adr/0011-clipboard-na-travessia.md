# ADR-0011 — O clipboard sincroniza na travessia, por um ajudante que roda como o usuário

**Status:** aceito · **Data:** 2026-09-18 · **Altera:** [05, §6](../05-windows.md) e
[02, §1.2](../02-arquitetura.md), onde "o clipboard pertence ao agente"

Duas decisões que nasceram juntas, de três fatos medidos na bancada.

## Os fatos

**O GNOME não avisa quando o clipboard muda.** Medido no GNOME 50.4 do Fedora da bancada:

| Operação | Resultado |
|---|---|
| publicar texto ou lista de arquivos (`wl-copy`) | funciona |
| ler (`wl-paste`) | funciona |
| ser avisado de mudança (`wl-paste --watch`) | *"Watch mode requires a compositor that supports the data-control protocol"* |

O aviso exige o protocolo `data-control`, e o GNOME não o expõe. O caminho que resta é o portal
`org.freedesktop.portal.Clipboard`, que só existe dentro de uma sessão de área de trabalho remota
aprovada pelo usuário — uma dependência de D-Bus e um diálogo de consentimento para ler o que o
próprio usuário copiou.

**O canal do agente é fechado para o usuário da sessão, nos dois sistemas.** No Windows é SDDL
para SYSTEM e Administradores; no Linux é `srw------- root`, medido em
`/run/inputremote/agent.sock`. É de propósito — o canal carrega injeção de entrada — e está certo.
Mas [02, §1.2](../02-arquitetura.md) punha o clipboard no agente, e [04, §4](../04-seguranca.md)
dizia que o agente do Linux roda "como o usuário da sessão". As duas coisas não cabiam juntas: o
agente do Linux nunca teria conseguido abrir o próprio canal.

**Liberar esse canal quebraria a entrada no Linux.** O serviço manda a injeção para o agente sempre
que há um agente pronto (`comandos_do_agente`), e só usa o `uinput` porque, no Linux, nenhum agente
conecta. Um agente de clipboard conectando tiraria teclado e mouse do `uinput` — que é o que funciona
no *greeter* e na tela de bloqueio.

## A decisão

**1. O clipboard é lido quando o controle sai desta máquina.** A sessão já emite
`Notice::ControlMoved { remote: true }` nesse instante, nos dois papéis. O serviço traduz isso em
`Aviso::LerClipboard`; quem cuida do clipboard lê e, se mudou, oferece ao par.

É o momento natural do uso: copia-se na máquina A, leva-se o cursor até B, cola-se em B. E tem uma
vantagem que o aviso por cópia não tem: **só atravessa o que o usuário leva junto.** Copiar uma pasta
de 2 GB e nunca colá-la do outro lado não custa nada.

Onde o sistema avisa — Windows, e compositores Wayland com `data-control` —, o aviso também é usado,
e a mesma cópia não sai duas vezes (`Eco`, com o segundo resumo). `Pedido::SincronizarClipboard`
dispara o mesmo gatilho à mão, para um atalho de teclado ou um item de bandeja.

**2. Quem cuida do clipboard é um ajudante que roda como o usuário**, `inputremote-agent --clipboard`,
falando pelo **canal de controle** — o mesmo da interface, com o portão por credencial que já existe.

*Revisto em 2026-09-19 ([log 40](../logs/40-o-ajudante-que-ninguem-subia.md)).* Quem **sobe** o
ajudante não é mais o login. Na primeira versão ele nascia pela chave `Run` (Windows) e pelo
autostart do XDG (Linux), que só valem ao entrar na sessão: instalar ou atualizar com o usuário
dentro deixava copiar e colar parado, sem aviso, até o próximo login — e um ajudante que morresse não
voltava. Agora:

- **Windows:** o serviço o lança na sessão de console, com o token de quem entrou
  (`WTSQueryUserToken`), e de novo sempre que nenhum estiver ligado por 12 s (`ir-daemon/zelador`).
- **Linux:** unidade do `systemd` do usuário (`inputremote-clipboard.service`, `Restart=always`,
  parte de `graphical-session.target`), que o pacote (re)inicia nas sessões abertas.
- **Nos dois:** o ajudante se apresenta com `Pedido::AcompanharClipboard`, e o serviço o conta — é a
  contagem que decide relançar, e que o diagnóstico mostra. Uma trava de arquivo faz um segundo
  ajudante do mesmo usuário sair.

O clipboard é dado do usuário, na sessão do usuário. Não há motivo para ele passar por um processo
com mais autoridade que isso.

## O que isso resolve de uma vez

- O agente do Linux não precisa de canal nenhum que o usuário não alcance.
- A injeção no Linux fica onde está, no `uinput`, sem risco de ser desviada.
- No Windows, clipboard deixa de exigir SYSTEM.
- O ajudante não pode pedir nada que o usuário não pudesse pedir pela janela.
- Para arquivos, **nenhum pedido novo**: oferecer é `Pedido::EnviarArquivos`, e publicar o que chegou
  é reagir ao `Aviso::Transferencia` concluído que já existia. O protocolo ganhou um aviso só.

## O defeito de segurança que apareceu no caminho

Ao ligar o ajudante a `Pedido::EnviarArquivos`, ficou visível que o pedido fazia o **serviço** — root,
SYSTEM — ler os caminhos que o **chamador** pedisse. Qualquer membro do grupo `inputremote` mandaria
`/etc/shadow` para o par. Introduzido no commit `28492e7`, corrigido antes de sair em pacote:

- o `uid` de quem conectou (`SO_PEERCRED`) acompanha cada pedido até o motor de transferência;
- só entra o que **pertence** a quem pediu ou que **qualquer um** leria, com todas as pastas acima
  atravessáveis — um `0644` dentro de um `/root` fechado não passa;
- a conferência final é feita **no descritor já aberto** (`fstat` e o caminho real em
  `/proc/self/fd`), e não no caminho: trocar uma pasta por vínculo simbólico entre o manifesto e a
  leitura não leva a leitura para outro lugar;
- no Windows o serviço (SYSTEM) identifica o cliente do *pipe* depois do primeiro pedido lido
  (`ImpersonateNamedPipeClient`, só para capturar o token, e `RevertToSelf` na mesma hora) e, para
  cada arquivo **já aberto**, pergunta ao próprio Windows se aquele usuário poderia ler
  (`GetSecurityInfo` do handle + `AccessCheck`). A ACL inteira decide — herança, grupos, negações —,
  e nada da regra é reimplementado. Se a identificação falhar, o serviço recusa.

O token é de nível **identificação**, o padrão com que o cliente abre o *pipe*: dá para perguntar,
não dá para agir como o usuário. Pedir nível de personificação ao cliente seria desnecessário e
perigoso — um programa que registrasse um *pipe* com o nosso nome antes do serviço poderia agir como
quem conectasse (*pipe squatting*).

Ver `ir_files::permissao` e `ir_acesso::identidade`.

## O que se perde, e é aceito

- **Arquivos grandes chegam depois da travessia, e não antes.** Quem atravessa e cola na hora pode
  colar o conteúdo anterior enquanto o novo ainda está chegando. No Windows o aviso por cópia manda
  antes; no GNOME não há como.
- **Um processo a mais na sessão.** Pequeno, sem janela, e sem ele a cópia não atravessa.
- **Colar no Linux depende do `wl-clipboard` instalado**, declarado no pacote.

## Texto

Vai pelo canal 4 da sessão, em qualquer portador: `Pedido::OferecerTexto` do ajudante, oferta,
pedido, pedaços do tamanho do menor portador e conferência por BLAKE3 do outro lado, que chega ao
ajudante como `Aviso::TextoRecebido`. Até 256 KiB (`MAX_CLIPBOARD_TEXT_OFF_TCP`); acima disso o texto
não atravessa, e o ajudante registra só tipo e tamanho. Provado na bancada nos dois sentidos, com
218 KB em 2,8 s ([log 35](../logs/35-o-texto-pelo-canal-4.md)).

## O que ainda não foi provado

- A identificação do cliente com o serviço **instalado**, como SYSTEM: o mecanismo tem teste com um
  *pipe* de verdade e com uma ACL que nega, mas o serviço de bancada roda sem elevação.
- Que o Nautilus cola a partir de `text/uri-list` — é o tipo padrão do XDG, mas o GNOME prefere
  `x-special/gnome-copied-files`, e só um teste com a mão no teclado responde.
