# ADR-0014 — Controle simétrico: quem mexe, manda

**Status:** aceito · **Data:** 2026-09-24 · implementado ([log 51](../logs/51-quem-mexe-manda.md))

## O problema

Cada computador tem um papel fixo: um "tem o teclado", o outro "é controlado". Quem está no
computador controlado não consegue usar o próprio mouse para ir ao outro lado; precisa abrir as
Preferências e trocar o papel. O protocolo 5 fez os dois papéis combinarem sozinhos
([log 46](../logs/46-os-papeis-que-combinam-sozinhos.md)), mas o papel continua existindo, e continua
sendo um conceito que o usuário precisa entender antes de usar.

O pedido: **os dois teclados e mouses controlam o outro**, sem ninguém ser "o dono", do jeito mais
natural possível. É o modelo do *Mouse Without Borders* da Microsoft e do *Logitech Flow*.

## O que a investigação encontrou

**O núcleo já tem as duas metades numa sessão só.** `session/server.rs` é "mandar a entrada daqui
para o par"; `session/client.rs` é "injetar o que o par manda". O papel (`config.role`) só escolhe
qual das duas roda: 31 usos em 8 arquivos da sessão, quase todos da forma "se sou servidor, faço
isto". [`Phase::Engaged`](../../crates/ir-session/src/phase.rs) já documenta que significa coisas
diferentes em cada papel — é exatamente a direção que o papel fixa.

**As duas plataformas já sabem capturar e injetar ao mesmo tempo, sem laço:**

| | Captura | Injeção | Distingue o próprio do injetado |
|---|---|---|---|
| Windows | ganchos do agente, sempre instalados | `SendInput` do agente | sim: `LLMHF_INJECTED`/`LLKHF_INJECTED` são ignorados (`windows/hooks.rs`) |
| Linux | `evdev`, com o cursor conduzido ([log 50](../logs/50-o-cursor-que-o-servico-conduz.md)) | `uinput` | sim: os dispositivos `InputRemote *` ficam fora da captura |

Esse era o risco de fundo — o computador A injeta no B, o B captura a própria injeção e devolve ao A
— e ele já está resolvido nos dois lados.

**O que não existe:** a regra de quem tem o controle quando os dois mexem, e uma mensagem para o
lado controlado **retomar** o controle sem atravessar a borda.

## A decisão

### 1. Não há papel. Há quem está usando agora.

A sessão deixa de ter `Role`. Cada máquina está em um de três estados, **dos dois lados ao mesmo
tempo coerentes**:

| Fase | Aqui | No par |
|---|---|---|
| `Ready` | cada um usa o próprio | `Ready` |
| `Sending` | a entrada daqui vai para o par | `Receiving` |
| `Receiving` | o par está usando esta tela | `Sending` |

`Phase::Engaged` virou `Sending`/`Receiving`, com a direção no tipo. A transição `Livre → Mandando` é a de hoje (o
ponteiro atravessa a borda); a de volta também (a borda oposta do lado que recebe).

### 2. Quem mexe por último, manda

- **Atravessar** continua sendo o gesto principal: de qualquer um dos dois lados, encostar na borda
  leva o controle ao outro.
- **Mexer o próprio mouse ou teclado no computador que está `Recebendo` retoma o controle na hora**
  (`Control::Reclaim`, nova). O outro lado solta a supressão e fica com o cursor onde saiu. É o que
  faz duas pessoas, cada uma na sua mesa, usarem os dois computadores sem combinar nada.
- **Contra tremida:** um toque na mesa não retoma. Retoma um clique, a roda, uma tecla que não seja
  só um modificador, ou ponteiro que andar mais de 20 px em 300 ms. E nos primeiros 150 ms depois de
  receber o controle, nada retoma — é o intervalo em que a mão de quem atravessou ainda está chegando.
  Um modificador sozinho (Ctrl, Alt, Shift) não retoma: é o começo de um atalho, e retomar nele faria
  o resto de Ctrl+Alt+Shift+Espaço devolver o controle ao outro.

### 3. O teclado vai junto com o cursor

Sem regra própria: as teclas vão para a máquina onde o cursor está. Uma tecla apertada no instante
em que o controle muda é solta do lado que perde (`ReleaseAll` e o `StateSnapshot` de sempre) — a
mesma rede de segurança de hoje contra tecla presa.

### 4. A posição do outro computador é uma só

"O Fedora fica à direita" num lado é "o Windows fica à esquerda" no outro. Qualquer um dos dois pode
mudar na tela; vale a mudança mais recente, pelo mesmo mecanismo de horário do protocolo 5, e o
outro lado passa a mostrar a oposta.

### 5. A preferência que sobra é de política, não de papel

Em Preferências, o cartão "Papel deste computador" vira **"Quem pode controlar"**:

- **Os dois** (padrão);
- **Só este controla o outro** — o computador do outro lado nunca vem para cá;
- **Só o outro controla este** — o de hoje "é controlado".

As duas últimas existem para quem precisa delas (um computador numa bancada que não deve ser
comandado de fora), e nenhuma aparece no caminho de quem só quer usar.

### 6. Compatibilidade: protocolo 6, e só ele

`Control::Reclaim` e `Capabilities::declines_control` entram no protocolo 6, `Control::Role` sai, e
`EdgeConfig` passa a levar o horário da escolha.

Na proposta, um par da versão 5 faria a sessão voltar aos papéis. Na implementação, a versão mínima
subiu para 6: manter os dois modelos vivos na sessão dobraria cada regra de direção, e não há versão
lançada — os dois computadores do usuário atualizam juntos. Um par antigo é recusado na negociação,
com "o outro computador tem uma versão mais antiga".

## O que isto custa

- **Linux:** o mouse e o touchpad ficam **sempre** com o serviço (a condução do log 50), também
  quando o outro lado controla este. Os gestos de três dedos do GNOME ficam indisponíveis o tempo
  todo, e não só com o teclado aqui. O caminho que devolve isso é o portal `InputCapture`, que
  continua na Etapa 6.
- **Windows:** nada novo — o agente já captura e injeta sempre.
- **Duas pessoas ao mesmo tempo** disputam o controle pelo "quem mexe por último". É o comportamento
  esperado nesse modelo; "borda travada" continua servindo para quem quer impedir a travessia.
- **Tela de bloqueio:** sem mudança. A política de cada máquina decide se aceita ser controlada ali
  ([04, §6](../04-seguranca.md)); o "Ctrl+Alt+Del no outro" funciona a partir de qualquer lado.

## Plano

1. **Núcleo** (`ir-session`, sem E/S): a direção no lugar do papel, `Reclaim` com a regra contra
   tremida, e a borda simétrica. Todo testável com duas sessões reais, como os testes de hoje.
2. **Protocolo 6**: `Control::Reclaim`, vetores gravados.
3. **Serviço e agente**: captura e injeção abertas sempre; no Linux, a condução também quando o par
   controla este.
4. **Janela**: o cartão "Quem pode controlar", a posição do outro editável dos dois lados, e o fim
   das frases que falam em "dono do teclado".
5. **Bancada**: atravessar e retomar dos dois lados, com Windows e Linux, e as teclas soltas na troca.

## Alternativas descartadas

- **Manter papéis e trocar sozinho quando o controlado mexe** — é o mesmo custo de implementar, e
  deixa o conceito de papel na tela sem motivo.
- **Controle simultâneo, os dois cursores ativos na mesma tela** — dois cursores num sistema que só
  desenha um; nenhum dos dois sistemas suporta isso sem uma camada própria de desenho.
