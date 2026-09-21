# A cópia que se repetia, o 0% que não andava, e a janela que não contava nada

**Data:** 2026-09-20

**Itens:** Etapa 8 — copiar e colar de ponta a ponta; Etapa 9 — a janela.

**O que foi feito:** três relatos do mesmo uso real, e os três são o mesmo assunto — o produto não
contava o que estava fazendo, e quem usa preencheu o silêncio apertando Ctrl+C de novo.

## As causas, pelos registros das duas máquinas

1. **Ctrl+C repetido copiava de novo, inteiro.** No registro do Linux, um envio de 2,2 GB terminou
   às 19:05:41.362 e o **mesmo** envio recomeçou 19:05:41.369 — 7 ms depois. Não era o rádio nem a
   rede: a fila de pedidos era ilimitada, e cada pedido virava uma cópia. O segundo pedido nasceu
   porque a guarda de eco (`ir_clip::Eco`) esquece o que ofereceu quando **publica** algo recebido
   do par — o que é correto para o clipboard e péssimo para arquivos de 2 GB.
2. **A barra ficava em 0% até o fim.** Quem recebe só contava duas vezes: ao aceitar o manifesto e
   ao concluir. Quem envia contava a cada arquivo — numa pasta com um arquivo de 2 GB, isso é a
   mesma coisa que não contar. A foto do usuário mostra "0% de 2,1 GB" com a cópia andando.
3. **A janela não dizia nada sobre velocidade nem sobre o que já foi copiado.**

## O que mudou

**Uma cópia de cada vez** (`ir-transferencia/src/fila.rs`), com três regras que agora são testadas:

- a mesma coisa pedida de novo é aceita e **não** vira outra cópia — é o Ctrl+C repetido de quem não
  viu retorno;
- coisa **diferente** cancela a que está indo: o `Cancel` vai pelo protocolo, e o outro lado apaga o
  que já gravou, porque a montagem dele só vira arquivo no fim;
- entre dois pedidos novos durante uma cópia, só o último espera.

**O andamento anda** (`ir-transferencia/src/passo.rs`): os dois lados contam a cada 200 ms, além do
fim de cada arquivo. Cinco avisos por segundo é o que se lê sem borrar, e não é ruído no canal de
controle — um despejo de dez mil blocos vira cinquenta avisos, não dez mil.

**A janela conta o que está acontecendo** (`ir-ui/src/historico.rs`):

- a linha da cópia passa a dizer quanto de quanto e a porcentagem — "pasta-B · 30,0 MB de 100,0 MB ·
  33%" — com a **velocidade** ao lado, medida entre avisos por média móvel;
- "Cópias recentes" abre a lista das últimas dez: nome, para que lado foi e como terminou. É a
  resposta para "aquilo copiou mesmo?", que é uma pergunta de **depois**;
- o tráfego da sessão fica ao lado, para "quanto isto usou da minha rede?".

**O aviso do canto não pisca**: com andamento cinco vezes por segundo, reabrir a janelinha a cada
aviso a faria piscar e devolver o foco sem parar. Aberta, só o texto muda.

## A prova

Testes novos: as três regras da fila, uma a uma; o relógio do andamento (o primeiro aviso passa na
hora, o intervalo segura os seguintes, e dez mil blocos não viram dez mil avisos); a velocidade (a
primeira medida não inventa número, 10 MB em 1 s são 10 MB/s, a média não salta, e outra cópia no
mesmo velocímetro não vira taxa negativa); a lista (só o que terminou, a mais nova primeiro, não
cresce sem fim) e o tráfego somando os dois sentidos.

E dois testes de ponta a ponta, com o canal de dados inteiro entre duas máquinas em processo
(`tests/uma_copia.rs`): pedir a mesma cópia três vezes seguidas produz **uma** conclusão; e copiar
outra coisa no meio cancela a anterior, entrega a nova, e não deixa sobra no destino.

**Bancada**, com **dois serviços de verdade** nesta máquina (`IR_DATA_DIR`, `IR_CONTROL_ENDPOINT` e
`IR_AGENT_ENDPOINT` sobem uma segunda instância sem administrador), pareados pela rede e trocando
uma pasta de 1,2 GB — o Windows tinha sido desinstalado, e a outra máquina não estava disponível:

```text
enviando arquivos itens=2 total=1258291200          ← o primeiro Ctrl+C
esta cópia já está indo; não vai de novo            ← o segundo
esta cópia já está indo; não vai de novo            ← o terceiro
cópia cancelada: o usuário copiou outra coisa       ← a outra pasta chegou
enviando arquivos itens=2 total=4194304             ← e assumiu
envio concluído e conferido pelo destino bytes=4194304
```

E o andamento, que era o que ficava em 0%: `0%`, `5%`, `21%`, `46%`, `82%` — 33 avisos numa cópia de
1,2 GB, e não um por bloco. Nenhuma montagem temporária sobrou no destino da cópia cancelada.

Uma armadilha para quem repetir: a ferramenta `controle enviar` **fica escutando até a cópia
terminar**, então três chamadas seguidas no mesmo terminal são serializadas — e nada se sobrepõe.
Cada pedido precisa de um processo próprio. Foi o que fez a primeira bancada "provar" que o defeito
continuava.

**Verificação:** 884 testes no Windows; `verificar.sh` verde no container do Fedora; `cargo xtask
check`.
