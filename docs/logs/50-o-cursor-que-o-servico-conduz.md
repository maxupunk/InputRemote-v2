# O cursor que o serviço conduz

**Data:** 2026-09-23

**Itens:** Etapa 6 — captura no Linux.

**O que foi relatado:** com o Windows já recebendo o ponteiro ([log 49](49-o-windows-que-recusava-tudo.md)),
a travessia do Linux para o Windows continuava vindo **no meio da tela** — com o touchpad e com um
mouse USB, depois de atualizar e de reconectar.

## A causa

As correções dos logs 48 e 49 ajustavam sintomas de uma causa só: **o Wayland não conta a ninguém
onde o cursor está**. O serviço ouvia os deslocamentos crus enquanto o GNOME movia o cursor com a
aceleração dele, e cada um andava na sua velocidade. Nenhuma escala fecha essa diferença — ela
depende da velocidade do gesto. E o que devia realinhar os dois, a volta do par pondo o cursor na
borda (`Command::WarpPointer`), era ignorado no Linux (`warp_pointer` não fazia nada). A diferença,
uma vez aberta, ficava para sempre.

## O que mudou: o serviço conduz o cursor

Com o teclado no Linux, o serviço deixa de adivinhar onde o cursor está e passa a **pô-lo** lá:

- a captura toma o mouse e o touchpad também com o controle local (`Capturer::conduzir_o_cursor`),
  e o GNOME para de movê-los sozinho; o **teclado** continua com o GNOME;
- cada deslocamento move o modelo da sessão, e o serviço põe o cursor do GNOME exatamente nele, pelo
  ponteiro virtual absoluto — o mesmo pelo qual o Windows controla o Linux, provado desde o log 34;
- botões e roda, que a captura tomou, são repostos pelo mesmo caminho;
- a volta do par põe o cursor real na borda por onde ele voltou;
- a sessão move o ponteiro local também sem conexão ou em pausa — senão o cursor congelaria;
- uma sessão recriada (troca de papel, reconexão) parte de onde o cursor está, sem salto.

O cursor real passa a **ser** o modelo: a travessia acontece exatamente na borda, a qualquer
velocidade, e a semente "longe da borda" e o touchpad lento do log 49 deixam de ser necessários.

Para a sensação ser a de sempre, os deslocamentos passam por uma curva de aceleração no espírito da
`libinput` (`linux/aceleracao.rs`): fator 1 devagar, crescendo com a velocidade até 4×.

## A segurança

- **A condução tem prazo.** O serviço a renova a cada segundo; sem renovação por 3 s, a captura
  devolve o mouse e o touchpad ao GNOME sozinha. Um serviço travado não deixa a máquina sem mouse.
  Um serviço que morre solta tudo na hora — o `EVIOCGRAB` acaba com o descritor.
- Só liga com captura **e** injetor abertos; sem os dois, nada é tomado.
- Um dispositivo com teclas de letra (um teclado com touchpad embutido num único dispositivo) conta
  como teclado e não é tomado com o controle local; o ponteiro dele volta a ser aproximado.

## O que se perde, conscientemente

Com o controle local, os gestos do touchpad do GNOME (três dedos para as áreas de trabalho) e a
configuração de velocidade do mouse nas Configurações deixam de valer: quem move o cursor é o
serviço. Tocar para clicar e rolar com dois dedos continuam, pelo `linux/touchpad.rs`.

## Como foi verificado

- `actor/cursor/tests.rs` (seis testes): o cursor nasce no meio e segue o modelo sem par; para na
  borda; botão e roda são repostos; sessão recriada parte do cursor; controlado ou sem injetor não
  conduz; a volta do par põe o cursor na borda.
- `linux/captura.rs`: na supressão tudo é tomado; conduzindo, só o ponteiro; a condução vencida
  devolve o mouse. `linux/aceleracao.rs`: quatro testes da curva.
- O gravador da configuração e as funções de papel e borda foram para o `ir-configuracao`, e o
  `ir-daemon` voltou para baixo do teto de 2 500 linhas.
- 1 080 testes no Windows, 1 024 no container do Fedora; clippy e `xtask` limpos.
- No notebook: o registro mostra `injetor aberto` e `o serviço passa a conduzir o cursor daqui`, com
  a sessão de pé; a tela é 1920×1080 sem escala, a mesma da configuração.
