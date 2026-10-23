# ADR-0017 — No GNOME, o clipboard vai pelo portal, e não pelo `wl-clipboard`

**Status:** aceito · **Data:** 2026-10-23 · **Altera:** [ADR-0011](0011-clipboard-na-travessia.md)
(o backend do Linux e "o GNOME não avisa") e [06, §6](../06-linux.md) · ([log 66](../logs/66-o-clipboard-do-gnome-pelo-portal.md))

## O defeito

No GNOME, cada cópia e cada travessia deixavam um "InputRemote está pronto" na tela (ou só
"InputRemote", na tela de bloqueio) e mais um ícone do InputRemote no dock. Na captura do usuário
eram onze.

**A causa, medida no notebook (Fedora 44, GNOME 50.6):** o GNOME não expõe `data-control`. Sem ele, o
`wl-copy` e o `wl-paste` abrem uma superfície própria para ganhar o foco do teclado, e é isso que dá
acesso ao clipboard no Wayland. O ajudante roda em segundo plano, numa unidade do `systemd` do
usuário. O GNOME recusa o foco a quem está em segundo plano e avisa que o programa "está pronto". O
dock mostra a superfície como janela do InputRemote. E o `wl-copy` **fica vivo** de propósito, porque
é ele que serve quem colar: o ícone fica junto. No notebook havia **quatro `wl-copy` vivos**, nenhum
saído, embora só o último fosse dono da seleção.

## A decisão

**No GNOME, o ajudante usa o portal `org.freedesktop.portal.Clipboard`**, dentro de uma sessão
`org.freedesktop.portal.RemoteDesktop`. **Nos outros ambientes fica o `wl-clipboard`.** A escolha
é feita uma vez, por `XDG_CURRENT_DESKTOP` (`ir_clip::linux`).

- É a interface que o GNOME oferece para quem controla a máquina de longe, e não abre janela. No
  spike: zero notificações e zero ícones.
- **O GNOME passa a avisar quando o clipboard muda** (`SelectionOwnerChanged`). O que o
  ADR-0011 chamava de impossível no GNOME agora existe: a cópia sai no momento em que é feita, como
  no Windows, e a travessia continua como segundo gatilho.
- O KDE e o Sway não vão para o portal: lá o `wl-clipboard` funciona limpo, e o portal custaria um
  diálogo sem ganho nenhum.
- Um GNOME cujo portal ainda não tem o clipboard (anterior ao 45) fica com o `wl-clipboard`. Os
  ícones voltam, mas copiar continua funcionando.

A alternativa recusada foi o clipboard do X11 pelo XWayland (`xclip`). Ela troca um contorno por
outro, depende do XWayland estar de pé e não avisa mudança.

## O custo: um diálogo, uma vez

O portal só entrega o clipboard numa sessão que a pessoa aprova no diálogo de acesso remoto do
GNOME. É o preço que o ADR-0011 recusou. Ele é aceito agora por duas razões: o defeito acima é pior,
e o diálogo aparece **uma vez**. O `Start` devolve um `restore_token`. O ajudante o guarda em
`$XDG_STATE_HOME/inputremote/portal-clipboard`, e as sessões seguintes são restauradas sem
diálogo.

Três fatos sobre essa persistência foram tirados do código do `xdg-desktop-portal` 1.22 e do
`xdg-desktop-portal-gnome` 50. O relatório que antecedeu esta decisão não os trazia:

1. **Pedir só o clipboard não é lembrado.** `serialize_session_as_restore_data` não devolve nada
   quando a sessão não compartilha dispositivo nem tela. Com `types: 0`, o `Start` volta sem
   `restore_token`, como aconteceu no spike, e o diálogo voltaria a cada login. **O ajudante pede o
   teclado** (`types: 1`), o menor dispositivo, e nunca manda tecla por ele: quem injeta continua
   sendo o `uinput`, pelo serviço.
2. **A ficha é consumida ao ser apresentada.** O portal apaga a autorização guardada assim que
   recebe a ficha no `SelectDevices`, e a grava de novo só se o `Start` der certo. Se a tela estiver
   bloqueada, o GNOME recusa a sessão (*"Session creation inhibited"*) e a autorização se perde. **Então
   o ajudante não tenta com a tela bloqueada**: ele espera o `org.gnome.ScreenSaver` avisar que ela
   voltou.
3. **O portal não tem "listar os tipos".** Eles só chegam no aviso de mudança. Antes do primeiro
   aviso, a leitura tenta, um por um, os tipos que nos interessam.

Se a pessoa recusar o diálogo, ou desligar nele o clipboard, o ajudante não pergunta de novo até
reiniciar, porque perguntar em laço seria insistir contra uma resposta dada. A sessão fechada pelo
GNOME é reaberta (com a ficha, sem diálogo).

## O que muda no resto

- `ir-clip` ganha o `zbus` no Linux: o mesmo do `ir-recado` e do Slint, sem biblioteca nova na
  árvore.
- O pacote passa a exigir `xdg-desktop-portal`, e continua exigindo `wl-clipboard` para os outros
  ambientes.
- Publicar ganhou `x-special/gnome-copied-files` junto da `text/uri-list`: o Nautilus cola pelo
  primeiro. Era uma das perguntas abertas do ADR-0011.

## O que ainda não foi provado

- O ícone de "acesso remoto" que o GNOME pode mostrar na barra superior enquanto a sessão existe.
  Ele não aparece por D-Bus, e o GNOME não deixa fotografar a tela de fora. Só quem está diante do
  notebook vê.
- A sessão diante do bloqueio de tela: se o GNOME a fecha ao bloquear, o ajudante a reabre ao
  desbloquear. O caminho existe no código, mas não foi medido.
