# O touchpad que a captura não via

**Data:** 2026-09-23

**Itens:** Etapa 6 — captura no Linux.

**O que foi relatado:** com os pacotes do [log 47](47-o-servidor-que-nao-lia-o-teclado.md)
instalados, o Fedora como "Tem o teclado" continuava sem passar o ponteiro para o Windows.

## A causa, pela bancada

O registro mostrou que a unidade nova resolveu a abertura: na subida como servidor, o aviso de
"captura indisponível" sumiu. Os descritores do serviço mostraram o resto:

```
/proc/<pid>/fd → /dev/input/event3   AT Translated Set 2 keyboard
               → /dev/input/event4   ELAN076C:00 04F3:3245 Mouse
                  (event5, ELAN076C:00 04F3:3245 Touchpad — não aberto)
```

A captura só reconhecia como mouse o que manda **deslocamento** (`REL_X`/`REL_Y`). O touchpad do
notebook manda **a posição do dedo** (`ABS_X`/`ABS_Y`), e quem a transforma em ponteiro é o
compositor, pela `libinput`. A captura fica abaixo dele e nunca abriu o touchpad — o modelo de
ponteiro da sessão não andava, e a borda nunca era alcançada. Os testes da captura só tinham mouse.

## O que mudou

`ir-input/src/linux/touchpad.rs`, sem E/S e com oito testes:

- um dispositivo que **aponta** (`INPUT_PROP_POINTER` — uma tela de toque não), tem `ABS_X`/`ABS_Y` e
  `BTN_TOUCH` é aberto como touchpad;
- enquanto há dedo, a diferença de posição entre um relato e o seguinte vira deslocamento; encostar
  em outro ponto, ou mudar o número de dedos, recomeça dali, sem salto; a fração de pixel se acumula,
  para o movimento lento não sumir;
- a escala é a largura do eixo X: passar o dedo de ponta a ponta anda 1 920 pixels;
- dois dedos rolam, com o conteúdo acompanhando o dedo, como o GNOME e o Windows fazem no touchpad;
- um toque curto (até 180 ms) e parado é clique — esquerdo com um dedo, direito com dois; o clique
  físico da superfície segue o caminho de sempre, e o toque que o acompanha não clica de novo.

## Como foi verificado

- Os oito testes do touchpad e o resto da captura, no container do Fedora: 1 008 testes, clippy limpo.
- Na bancada: o serviço compilado com isto no lugar do binário do pacote
  (`/usr/bin/inputremote-daemon.pacote` guarda o original), e o touchpad passou a aparecer entre os
  descritores abertos — `event3`, `event4` e `event5`. A sessão voltou pela rota dupla.

## O que continua de fora

Sem a aceleração da `libinput`, o modelo de ponteiro da sessão e o cursor que o GNOME desenha andam
em velocidades diferentes. Com o controle aqui, a travessia pela borda acontece perto do ponto
certo, e não exatamente nele: às vezes é preciso continuar empurrando na borda, e
Ctrl+Alt+Shift+Espaço atravessa na hora ([06, §3.4](../06-linux.md)). O caminho exato continua sendo
o portal `InputCapture`.
