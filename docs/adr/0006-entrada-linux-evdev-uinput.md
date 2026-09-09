# ADR-0006 — Entrada no Linux: `uinput` para injetar, portal para capturar

**Status:** aceito · **Data:** 2026-09-09 · **Substitui:** nada

## Contexto

No Wayland, captura e injeção passam por portais que negociam com o compositor e com o
usuário. Na tela de login não existe usuário, não existe sessão e não existe portal — o
GDM roda um compositor próprio, isolado. O caminho correto para o uso normal é justamente
o indisponível para o requisito R1.

O v1 mantinha dois caminhos de injeção vivos (portais e `uinput`) com regras de escolha
entre eles, e ainda assim não cobria a tela de login, que estava fora de escopo.

## Decisão

Assimétrica, de propósito. Cada papel usa a tecnologia em que é forte:

| Papel | Caminho | Por quê |
|---|---|---|
| **Cliente** (injeta) | `/dev/uinput`, direto do serviço de sistema | funciona no greeter, na tela de bloqueio, em qualquer compositor, no X11 e no console |
| **Servidor** (captura) | portal `InputCapture` + `libei` | é o único mecanismo que sabe dizer "o ponteiro encostou na borda" |

A assimetria se justifica porque o servidor é, por definição, a máquina que está sendo
usada neste instante — ela está desbloqueada. O requisito da tela de bloqueio vale só
para o cliente.

Injeção é **sempre** `uinput`, em toda situação. A consulta ao `logind` serve para
alimentar a interface e a política de tela de bloqueio, não para escolher o método.

## Alternativas descartadas

**Só portais, nos dois papéis.** É o caminho "educado" e o que o Wayland pretende. Não
atende o requisito: sem sessão, sem portal, sem tela de login. Fica como modo opcional de
Fase 2, para quem recusar a instalação privilegiada — com a limitação escrita.

**Só `evdev` + `uinput`, nos dois papéis.** Tentador pela uniformidade, e resolveria
compositores sem `InputCapture`. Esbarra na detecção de borda: com `EVIOCGRAB` sabemos
quais eventos ocorreram, mas não onde está o cursor, e reconstruir a posição acumulando
deltas diverge do cursor real por causa da aceleração do compositor.

Existe uma saída — o serviço grava os dispositivos físicos **sempre** e passa a alimentar
o compositor por um dispositivo `uinput` absoluto próprio, tornando-se o dono da posição
do cursor. O preço é implementar a curva de aceleração local, e o risco é o ponteiro local
ficar diferente do que o usuário conhece. Fica registrado como o caminho da Fase 2 para
compositores sem `InputCapture` (§3.3 de [06](../06-linux.md)).

**`XTEST` / X11.** Fora de escopo do produto.

## Consequências

**Boas.**
- A tela de login e a de bloqueio funcionam em qualquer compositor, sem cooperação dele.
- Um caminho de injeção só, sem regra de escolha — simplificação direta sobre o v1.
- Se o agente de sessão morrer, a máquina continua servindo como cliente, inclusive
  bloqueada. É o comportamento desejado.
- `InputCapture` está maduro: integrado ao `xdg-desktop-portal` desde 1.21.0 e validado
  contra GNOME Wayland em 01/09/2026 — situação diferente da de quando o v1 foi escrito.

**Ruins, e aceitas.**
- `uinput` **contorna** o modelo de segurança de entrada do Wayland. É defensável porque
  o usuário instalou deliberadamente um serviço privilegiado para este fim, e porque
  o controle
  de quem pode pedir injeção está em [04, §5](../04-seguranca.md). Precisa estar escrito
  no README, não escondido.
- Duas tecnologias de entrada para manter, uma por papel.
- Compositores sem `InputCapture` não podem ser servidores na Fase 1.
- Risco de auto-recaptura: um cliente que também é servidor lê os próprios dispositivos
  virtuais e cria um laço. O filtro por dispositivo de origem é obrigatório desde o
  primeiro commit.
- A armadilha do atraso após `UI_DEV_CREATE` ([06, §2.2](../06-linux.md)) precisa ser
  tratada na subida do serviço, ou os primeiros eventos somem em silêncio.
