# Sem conexão, em silêncio

**Data:** 2026-10-26 · **Branch:** `feat/painel-android`

**Itens:** [PROGRESSO](../../PROGRESSO.md), copiar e colar.

**O que foi pedido:** "quando está offline o PC 1 com o 2 e faço uma cópia de arquivo, recebo 'a
cópia não atravessou', porém nem deveria ter iniciado a cópia e nem informado. Quando não tiver
conectado, não ficar tentando enviar arquivos e avisando coisas: deve ficar silencioso até
reconectar."

## O que havia

Um Ctrl+C de arquivo com o outro computador desligado entrava na fila "esperando a conexão" — com
cartão na janela — e, um minuto depois ([`PRAZO_PARA_VOLTAR`](../../crates/ir-transferencia/src/retomada.rs)),
virava "A cópia não atravessou", com notificação do sistema. Sem par, o pedido era recusado com
aviso ("não há par pareado"); com a porta do canal ocupada, também. Cada cópia feita com o outro
computador longe era um aviso de falha.

## O que mudou

- **Sem o outro computador ao alcance — o canal de arquivos fora do ar —, a cópia não começa.**
  `Pedidos::enviar` olha a marca do canal antes de pôr na fila: sem canal, nada entra, nenhum cartão
  aparece e nenhuma notificação sai; o registro do serviço diz, em `debug`, que a cópia não começou.
  A resposta ao agente é a recusa de sempre (`Falha::SemConexao`), que ele já tratava calado: marca a
  oferta para ir de novo na **próxima travessia** do cursor. Com o outro computador de volta, a cópia
  feita "no escuro" vai sozinha na primeira travessia — sem aviso nenhum no meio.
- **A cópia interrompida no meio não mudou**: ela começou com os dois conectados, a pessoa a viu
  andando, e recomeça sozinha se a conexão voltar em um minuto. Sumir calada seria pior que dizer que
  não chegou. Se a pessoa copia outra coisa enquanto ela espera, ela sai da fila (o clipboard agora é
  outro, e entregá-la na volta colaria a coisa errada) e o cartão dela diz que parou.
- **`Pedidos::ao_alcance()`**: se o canal está de pé — o que os testes esperam antes de copiar, como a
  pessoa espera o outro computador ligar.

## O que foi provado

- Os testes do canal de arquivos com duas máquinas de verdade: sem par, a cópia volta `false` e
  nenhum aviso sai em meio segundo; pareados, a mesma cópia passa sem reiniciar nada; com a porta
  ocupada, nada começa, e quando ela solta a cópia passa. A retomada da cópia interrompida, intacta.
- `clippy` e os testes do serviço, do agente e da transferência.

## O que falta

- Na bancada: desligar o outro computador, copiar um arquivo, ver que nada aparece; ligá-lo, passar o
  cursor, e colar do outro lado.
