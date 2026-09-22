# ADR-0012 — Rota dupla: Bluetooth e rede ao mesmo tempo, vale o que chegar primeiro

**Status:** aceito · **Data:** 2026-09-22 · **Altera:** [03, §2](../03-protocolo.md) (a regra de
portadores de entrada mutuamente exclusivos) e [01, §5](../01-visao-e-escopo.md) (a política única)

## O problema

A política única de [01, §5](../01-visao-e-escopo.md) escolhia **um** portador de entrada:
Bluetooth se existisse, senão a rede. Os dois falham de jeitos que o outro não falha:

- o **Bluetooth** sofre interferência em 2,4 GHz, e o rádio que trava continua "de pé" para o
  sistema — o que vai por ele simplesmente não chega, até o enlace cair por tempo;
- a **rede** tem os picos do Wi-Fi em economia de energia e os silêncios de mais de 10 s na troca
  de ponto de acesso ([log 24](../logs/24-a-borda-e-do-servidor.md)).

Com um portador só, qualquer dos dois vira atraso visível ou queda, e trocar de portador exigia
soltar tudo e refazer a sessão. O pedido de quem usa: com o meio de conexão no automático, mandar
por **os dois** ao mesmo tempo e descartar a cópia.

## A decisão

Com o meio no **automático** e os dois portadores de pé, a sessão fala por uma **rota dupla**:
cada quadro sai pelo Bluetooth **e** pela rede, e do outro lado vale o que chegar primeiro. Fixar
um portador desliga a rota dupla, como já desligava a degradação.

### Onde a cópia é descartada: na sessão

Cada portador tem o seu aperto de mão Noise, com chaves e contadores próprios: as duas cópias
de um mesmo quadro são **bytes cifrados diferentes**. Só depois de decifrar se vê que são a mesma
mensagem, e o que as identifica é o `(época, canal, sequência)` do quadro — que é da sessão, e
não de portador nenhum. Por isso o descarte mora no `ir-session`, e não no transporte: o
transporte teria de decodificar o protocolo, e o `ir-transporte` existe justamente para não
decidir nada.

### A peça central: a rota dupla é um datagrama

Dois caminhos juntos podem duplicar e reordenar, e cada um pode perder o que estava nele quando
caiu. É exatamente a garantia do UDP, que a sessão já tratava ([03, §4.1](../03-protocolo.md)):
sequência por canal, confirmação, retransmissão, fila de reordenação e descarte de repetição. A
rota dupla não precisou de mecanismo novo de entrega — precisou que a sessão tratasse a rota como
datagrama.

Por isso, desde a **versão 2** do protocolo, a sessão trata **toda** rota de entrada como
datagrama, inclusive o RFCOMM sozinho. É isso que permite a rota mudar sem refazer a sessão: a
garantia da encarnação não muda quando um portador entra ou sai. O custo sobre o RFCOMM é uma
confirmação de poucos bytes a cada 20 ms enquanto houver algo pendente.

### O que a rota muda, e o que não muda

- Um portador **entrar** na rota (ele subiu, ou a sessão ficou de pé) não refaz a sessão.
- Um portador **sair** da rota (caiu) não refaz a sessão, **não solta teclas** e não começa
  encarnação nova: o que estava em trânsito nele chega pelo outro, ou pela retransmissão.
- Só a queda do **último** portador encerra a sessão, e aí vale a regra de sempre: soltar tudo
  antes de qualquer outra coisa.
- O prazo de queda de 1 s passa a contar a **rota inteira**: a sessão só cai se os dois ficarem
  calados.

### Os endereços

Para manter os dois enlaces de pé, o serviço precisa do endereço do par em cada portador:

- o da **rede** vem do pareamento, do `peer_addr`, ou da descoberta — que acha o par pelo
  `MachineId`, derivado da chave fixada;
- o do **rádio** não tem como ser achado pela rede. O par o conta numa mensagem nova,
  `Control::Reach`, assim que a sessão fica de pé, e o serviço o grava no par
  (`PinnedPeer.radio`).

Os dois lados passam a discar o portador que falta ao mesmo tempo. A regra de quem disca
([`turno`](../../crates/ir-crypto/src/turno.rs)) saiu do `ir-net` para o `ir-crypto` e vale
agora para o rádio também.

## Regras que não podem ser quebradas (cada uma tem teste)

1. **A cópia é aplicada uma vez só.** Teclado, controle, retorno e texto passam pela detecção de
   repetição; o **ponteiro** passa pelo filtro de sequência de [03, §4.2](../03-protocolo.md), que
   existia na especificação e faltava no código — sem ele, o movimento relativo andava o dobro.
2. **Só entra na rota um enlace com a mesma chave fixada.** Um par de outra identidade num dos
   portadores é recusado pelo serviço antes de a sessão saber dele.
3. **A proteção contra repetição continua valendo.** Cada portador mantém a sua janela Noise, e a
   sessão impede aplicar a mesma mensagem duas vezes ([04, §2](../04-seguranca.md)).
4. **Durante um pareamento, só o enlace do pareamento o conclui.** Com a rota dupla o outro lado
   disca o segundo portador sozinho; antes, qualquer enlace que subisse com um código na tela
   concluía o pareamento e gravava a chave — sem a confirmação do usuário, e sem conferir se a
   chave era a do código. Agora outro portador ou outra chave é recusado, e a queda dele não
   desfaz o pareamento em curso ([04, §3.2](../04-seguranca.md)).
5. **Todo quadro cabe no menor portador.** O teto do RFCOMM (512 B) já era o limite dos pedaços
   do clipboard.

## A fila do rádio

Sob interferência, o RFCOMM para de escoar e a fila de envio cresce; quando ele volta, despejaria
segundos de quadros velhos na frente dos novos. Duas mudanças no `ir-bt`:

- o endpoint descarta, **antes de cifrar**, o quadro que esperou mais de 250 ms na fila (um quarto
  do prazo de queda, que é também o teto de retransmissão). Depois de cifrar não dá: o contador do
  enlace é implícito, e pular um quadro cifrado derrubaria o enlace;
- a ponte do Windows, que aceitava escrita sem limite, passou a ter no máximo 4 quadros em voo,
  para a espera voltar ao endpoint, onde o descarte age.

## Consequências

- **Versão 2 do protocolo, nas duas pontas** (`MIN_SUPPORTED = 2`). Uma ponta da versão 1 sobre
  RFCOMM não confirmaria nada, e a janela da versão 2 encheria até derrubar a sessão a cada
  segundo, em silêncio. Recusar na negociação diz o motivo. Não havia versão 1 lançada.
- **A tela mostra "Bluetooth + Rede local"** e o motivo "Bluetooth e rede local juntos". O
  diagnóstico e o registro mostram, a cada minuto, por qual portador cada quadro novo chegou
  primeiro e há quanto tempo cada um não é ouvido — o dado que diz se a rota dupla está pagando o
  que custa.
- **O Bluetooth deixa de ser "o portador" e vira reserva quente** quando a rede é boa: no Wi-Fi de
  5 GHz a rede vence quase sempre. A latência de cada quadro passa a ser a **menor** das duas.
- **O Linux não reduz o buffer do socket RFCOMM.** O `bluer` não expõe `SO_SNDBUF`, e o `unsafe`
  do `ir-bt` é restrito ao Winsock ([09, §4](../09-padroes-de-codigo.md)). O descarte por idade
  age antes do kernel; o que já está no buffer do kernel sai atrasado, e a cópia pela rede já
  chegou.
- **Crates mudaram de forma** para caber no teto de tamanho: a confiabilidade dos canais saiu do
  `ir-session` para o `ir-confiabilidade` (puro); a subida dos portadores e o alcance do par saíram
  do `ir-daemon` para o `ir-transporte`; e a configuração e a identidade persistentes saíram do
  `ir-daemon` para o `ir-configuracao`.

## Alternativas descartadas

- **Descartar a cópia no transporte.** Exigiria o transporte decodificar o protocolo e conhecer a
  época da sessão; quebra a responsabilidade única do `ir-transporte`.
- **Um portador por vez, com troca rápida.** A troca pede soltar tudo e refazer a sessão, e é
  exatamente o que a interferência não pode custar.
- **Duplicar só teclado e mouse.** A regra "o que vai pela rota vai pelos dois" é mais simples de
  provar; o texto do clipboard pesa pouco no rádio e o descarte por idade protege a entrada.
