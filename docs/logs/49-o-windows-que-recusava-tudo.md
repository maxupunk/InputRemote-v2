# O Windows que recusava tudo em silêncio, e a travessia que vinha antes da borda

**Data:** 2026-09-23

**Itens:** Etapa 5 — injeção no Windows; Etapa 6 — captura no Linux.

**O que foi relatado:** com o touchpad já capturado ([log 48](48-o-touchpad-que-a-captura-nao-via.md)),

1. o ponteiro saía do Linux para o Windows **antes** de chegar à borda, com a tela dizendo
   "Controlando SAMSUNG-MAXUEL";
2. o Linux dizia "Controlando", mas o cursor do Windows **ficava parado**.

## 2. O cursor parado: o `SendInput` sem o direito que exige

O registro do Linux mostrou a travessia acontecendo e voltando, várias vezes — o controle ia. O do
Windows é restrito a SYSTEM e administradores desde a varredura, então a falha foi reproduzida aqui,
com o mesmo injetor do agente num teste (`o_ponteiro_injetado_chega_a_area_de_trabalho`, `#[ignore]`):
desktop de entrada `Default`, thread `Default`, e **`Rejected`** — o `SendInput` devolvia 0.

O `SendInput` exige que o desktop da thread tenha sido aberto com `DESKTOP_JOURNALPLAYBACK`. As
threads por desktop da varredura ([log 45](45-a-varredura-implementada.md), ADR-0008) abriam cada
desktop só com leitura, escrita e troca. Antes delas, o injetor usava o desktop do próprio processo,
com acesso total — por isso funcionava. **Toda** injeção no Windows passou a ser recusada, e nenhum
teste automático injeta de verdade.

Com o direito, o mesmo teste levou o cursor a `(419, 175)` e `(2099, 659)` — o que se pediu, no
desktop virtual de 3440×1440. `o_desktop_abre_com_o_direito_que_o_sendinput_exige` trava o direito
sem mexer no cursor.

**E ninguém ficou sabendo.** A recusa ia só para o registro, uma linha por evento. Agora, enquanto
houver recusa na área de trabalho, a tela do Windows diz "Este computador não consegue receber o
teclado e o mouse do outro…", e o aviso some sozinho 5 s depois da última; o registro anota só a
mudança.

## 1. A travessia antes da borda: o modelo à frente do cursor

No Linux a sessão não sabe onde o GNOME desenhou o cursor: ela acompanha um modelo, alimentado pelos
deslocamentos da captura. Ele começava numa posição inventada e andava, no touchpad, mais rápido
que o cursor real — chegava à borda primeiro, e atravessava com o cursor no meio da tela.

As duas mudanças fazem o erro cair sempre para o lado seguro:

- **o modelo começa encostado no lado oposto à borda** (`Session::seed_pointer_away_from_edge`), e
  não no meio;
- **o touchpad anda 1 000 pixels por passada**, e não 1 920 — mais devagar que o cursor do GNOME.

O modelo só chega à borda depois do cursor real: atravessar é levar o cursor até a borda e continuar
empurrando. A primeira volta do par realinha os dois, porque a volta põe o modelo na borda em que o
cursor real está. Ctrl+Alt+Shift+Espaço continua atravessando na hora.

## Como foi verificado

- O teste do injetor contra o cursor desta máquina, antes (`Rejected`) e depois (o cursor no ponto
  pedido).
- `sem_posicao_real_o_ponteiro_comeca_do_lado_oposto_a_borda` (`ir-session/tests/edge.rs`),
  `a_passada_inteira_anda_menos_que_uma_tela` (touchpad) e
  `a_recusa_na_area_de_trabalho_aparece_na_tela_e_some_quando_para` (serviço).
- 1 074 testes no Windows, 1 011 no container do Fedora; clippy e `xtask` limpos.
- O serviço novo instalado à mão no notebook, com a sessão de pé.

**O Windows só se corrige com o MSI novo** — é o agente instalado que injeta.
