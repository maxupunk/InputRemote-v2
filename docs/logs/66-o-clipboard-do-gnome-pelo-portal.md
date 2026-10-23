# O clipboard do GNOME pelo portal: sem ícones no dock, e com aviso de mudança

**Data:** 2026-10-23

**Itens:** [ADR-0017](../adr/0017-clipboard-do-gnome-pelo-portal.md);
[PROGRESSO](../../PROGRESSO.md), Etapa 8.

**O que foi pedido:** avaliar se o relatório `PORTAL_CLIPBOARD.md` (escrito numa sessão anterior,
com diagnóstico e spike) estava certo e completo, e implementar.

## O defeito, confirmado

No GNOME, cada cópia e cada travessia deixavam um "InputRemote está pronto" na tela e mais um ícone
do InputRemote no dock. O relatório atribuía isso ao `wl-clipboard`: sem `data-control`, o `wl-copy` e
o `wl-paste` abrem uma superfície para ganhar foco, e o GNOME avisa e põe a superfície no dock. A
medição no notebook confirmou: havia **quatro `wl-copy` vivos** (sete minutos depois), nenhum saído,
embora só o último fosse dono da seleção. Depois da troca, **zero**.

## O relatório: certo no diagnóstico, incompleto onde ia doer

O diagnóstico e a escolha do portal se sustentam. O relatório errava, ou não dizia, em quatro pontos
que quebrariam a implementação. Os quatro saíram do código do `xdg-desktop-portal` 1.22 e do
`xdg-desktop-portal-gnome` 50, que estavam junto dos scripts do spike:

1. **`types: 0` nunca é lembrado.** O relatório recomendava pedir só o clipboard. Mas
   `serialize_session_as_restore_data` não devolve nada sem dispositivo nem tela, e o `Start` volta
   sem `restore_token`. O próprio registro do spike mostra isso (`{'devices': 0, 'clipboard_enabled':
   True}`, sem ficha), e o PermissionStore do notebook não tinha nenhuma entrada com clipboard. O
   diálogo voltaria a cada login. **Corrigido:** pede-se o teclado (`types: 1`), que nunca é usado.
2. **A ficha é consumida ao ser apresentada.** O portal apaga a autorização no `SelectDevices` e só
   a regrava se o `Start` der certo. Tentar com a tela bloqueada (*"Session creation inhibited"*, que
   o relatório registrou) apagaria a autorização. **Corrigido:** o ajudante espera o
   `org.gnome.ScreenSaver` avisar que a tela voltou, antes de tentar.
3. **O portal não lista os tipos.** O relatório mandava ler `text/uri-list`, `text/plain` ou
   `image/png` conforme o caso, mas não dizia como saber o que há. Os tipos só chegam no
   `SelectionOwnerChanged`. **Feito:** guardam-se os do último aviso e, antes do primeiro, tenta-se
   um por um.
4. **O `Clipboard` e o `Vigia` são objetos separados, em threads diferentes**, e o
   `SelectionTransfer` chega a qualquer hora, enquanto o laço do ajudante está parado esperando o
   serviço. **Feito:** um `Portal` por processo, com duas threads próprias (o zelador da sessão e a
   escuta dos sinais), e as colagens servidas cada uma na sua thread.

E um ponto saiu da bancada: o texto que chegava do Windows **com a tela do notebook bloqueada** se
perdia, porque ainda não havia sessão. Agora ele fica guardado e é anunciado quando a sessão abre.

## O que foi feito

- `ir-clip/src/linux/` virou módulo, e a escolha é feita uma vez, por `XDG_CURRENT_DESKTOP`.
  `IR_CLIPBOARD=portal|wl-clipboard` força a escolha.
  - `portal/` atende o GNOME.
  - `wl.rs` é o backend de antes, para KDE e Sway.
  - Um GNOME sem o clipboard no portal (anterior ao 45) fica com o `wl-clipboard`.
- `portal/sessao.rs`:
  - `Registry.Register("inputremote")` antes de tudo, para o portal ligar a autorização ao `.desktop`
    instalado;
  - `CreateSession` → `SelectDevices` (teclado, `persist_mode: 2`, a ficha guardada) →
    `RequestClipboard` → `Start`;
  - a ficha fica em `$XDG_STATE_HOME/inputremote/portal-clipboard`;
  - o `Closed` é assinado antes do `Start`, e a sessão fechada é reaberta;
  - a recusa no diálogo para tudo até o ajudante reiniciar, sem perguntar em laço.
- `portal/mod.rs`:
  - `SelectionOwnerChanged` guarda os tipos e acorda o vigia. **O GNOME agora avisa a mudança**,
    como o Windows: a cópia sai na hora, e a travessia continua como segundo gatilho;
  - `SelectionTransfer` responde com `SelectionWrite`, os bytes e `SelectionWriteDone`;
  - a leitura (`SelectionRead`) tem prazo de 3 s, como o `wl-paste`.
- `portal/tipos.rs`:
  - o texto sai com cinco nomes (`text/plain;charset=utf-8`, `UTF8_STRING`, `text/plain`, `STRING`,
    `TEXT`);
  - os arquivos saem como `x-special/gnome-copied-files` e `text/uri-list`. O Nautilus cola pelo
    primeiro, e o ADR-0011 tinha isso como pergunta aberta;
  - a imagem sai como `image/png`.
- `zbus` no `ir-clip` (Linux): o mesmo do `ir-recado`. Pacote: `Requires: xdg-desktop-portal`.

## Na bancada (Fedora 44, GNOME 50.6)

O ajudante novo foi posto no lugar do instalado por um *drop-in* da unidade do usuário
(`~/.config/systemd/user/inputremote-clipboard.service.d/teste-portal.conf`, binário em `~/ir-e2e`):

- `clipboard pelo portal do GNOME`, com o portal na versão 1 do clipboard;
- os `wl-copy` foram de 7 para **0**;
- a tela estava bloqueada, e o ajudante **esperou**, em vez de gastar a autorização:
  `tela bloqueada: o clipboard do portal espera ela voltar`.

**Falta, e depende de alguém diante do notebook:**

- desbloquear e aprovar o diálogo uma vez;
- conferir que um segundo login não o mostra de novo;
- copiar e colar nos dois sentidos;
- olhar se aparece algum ícone de acesso remoto na barra superior.

Para conferir: `sh ~/ir-e2e/portal-teste.sh log`. Para voltar ao ajudante do pacote:
`sh ~/ir-e2e/portal-teste.sh desfazer`.

**Verificação:**

- Linux (contêiner, `verificar.sh`): clippy do workspace limpo, 1 312 testes passando e nenhuma
  falha. Depois da correção do texto guardado, foram repetidos o clippy e os testes do `ir-clip`.
- Windows: clippy e testes do `ir-clip` limpos; `xtask check` dentro das regras.
