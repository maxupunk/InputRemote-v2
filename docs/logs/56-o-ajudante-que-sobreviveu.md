# O ajudante que sobreviveu à atualização, e a promessa no clipboard

**Data:** 2026-10-02

**Itens:** [ADR-0011](../adr/0011-clipboard-na-travessia.md) — clipboard na travessia;
[05, §6](../05-windows.md) — o clipboard do Windows; [log 55](55-o-manifesto-que-nao-cabia.md).

**O que foi pedido:** com o pacote do log 55 instalado nos dois, copiar do Windows para o Linux
funcionava, e do Linux para o Windows não. Verificar na bancada; depois, fazer colar antes de a
cópia chegar.

## Do Linux para o Windows: o transporte estava certo

O registro do Fedora mostrava os envios **concluídos e conferidos** pelo Windows — inclusive uma
pasta de 1 805 itens, que só atravessa com o manifesto em partes do log 55. O que faltava era o
último passo: pôr no clipboard do Windows o que chegou. E, a cada envio, o Windows devolvia ao Linux
sempre os mesmos 6,7 MB — o conteúdo velho do clipboard dele, reoferecido.

Quem põe no clipboard é o ajudante de clipboard, e o que rodava no Windows era um processo de
**24/09**. A instalação troca o executável em disco sem encerrar o processo, e ele tinha atravessado
todas as atualizações desde então. Ao receber "cópia concluída", ele registrava *mensagem de IPC
malformada*, reconectava e seguia — onze vezes no dia. Encerrado, o serviço subiu o instalado, e a
pasta enviada pelo Linux apareceu no clipboard do Windows com o conteúdo certo.

A mensagem estava malformada **por causa do log 55**: `Fase::AguardandoConexao` entrou no meio do
enum, e o canal local é `postcard`, que numera as variantes pela posição. `Concluida` mudou de
número, e quem era de antes lia outra coisa.

## O que mudou

- **`AguardandoConexao` foi para o fim do enum**, e um teste trava o número de cada fase no fio
  (`o_numero_de_cada_fase_no_fio_nao_muda`). Variante nova no canal local vai no fim.
- **O ajudante sai sozinho quando o executável dele muda em disco** (`clipboard/atualizacao.rs`).
  Sair ao receber um quadro que não entende só pegava a mensagem que mudou de forma, e só quando ela
  chegava. Agora a pergunta vale para qualquer atualização: o arquivo no caminho ainda é o que este
  processo carregou? Se não for, ele sai, e quem zela pelo ajudante sobe o instalado. Visto
  funcionando na bancada: um ajudante novo contra o serviço de antes recebeu "concluída", reconheceu
  a outra versão, saiu, e o serviço subiu o instalado.
- A conversa com o serviço saiu do laço principal do ajudante para `clipboard/servico.rs`.

## Colar antes de a cópia chegar

Do Linux, a cópia só começa quando o mouse chega ao Windows — e quem atravessa para colar cola logo.
Colava o que havia antes: a cópia ainda estava a caminho.

### A primeira tentativa, e por que foi descartada

A renderização atrasada do Win32: pôr no clipboard um `CF_HDROP` sem dados e entregá-los quando
alguém colasse, esperando a cópia. Funcionou no papel e falhou na bancada por um motivo que a
documentação não conta: **o próprio Explorer lê todo conteúdo novo do clipboard** (para esmaecer
arquivos recortados, entre outras coisas). Ele pedia a promessa na hora e ficava esperando com o
clipboard **aberto** — e com ele aberto nenhum programa da máquina copiava nem colava, nem o Ctrl+V do
próprio Explorer, que é o que se queria. Medido: com a promessa pendente, 100% das aberturas do
clipboard falhavam, e quem o segurava era o `explorer.exe`.

### O que ficou: arquivos virtuais

O mesmo recurso da Área de Trabalho Remota para colar arquivos que ainda estão do outro lado
(`ir-clip/src/windows/promessa/`):

- quando a cópia é aceita, o serviço conta ao ajudante o que está chegando e onde
  (`Aviso::ArquivosChegando`, só para o ajudante — a janela não recebe os caminhos);
- o ajudante põe no clipboard um objeto OLE com `FileGroupDescriptorW` — nomes, pastas e tamanhos,
  que saem do manifesto na hora — e `FileContents`, um fluxo por arquivo;
- ao colar, o Explorer abre o próprio diálogo de cópia e lê cada fluxo, que devolve o que já está em
  disco e espera o resto. O clipboard só fica aberto para pegar o objeto; os bytes chegam com ele
  livre;
- quando a cópia termina, os arquivos de verdade (`CF_HDROP`) tomam o lugar dos virtuais, para os
  programas que só entendem a lista de caminhos.

As regras:

- **A última cópia vale.** Se a pessoa copiar outra coisa enquanto os arquivos chegam, eles não
  passam por cima da cópia nova. Antes passavam.
- **O fluxo abre, lê e fecha a cada leitura**, e depois da publicação lê de onde a entrega ficou.
  Medido: com um arquivo aberto dentro, a pasta da montagem não pode ser renomeada, nem com todo
  compartilhamento — segurar o arquivo impediria a publicação. E a publicação passou a repetir o
  `rename` negado por um instante, por até três segundos, o que também a protege do antivírus e do
  indexador.
- **A cópia que não chega desfaz a promessa**: o diálogo do Explorer mostra o erro, e o clipboard
  deixa de prometer. A que espera a conexão voltar mantém a promessa.
- **Ninguém daqui lê a promessa**: o ajudante não lê o clipboard enquanto ele é do próprio processo.
- **Fica de fora** a imagem (vira imagem no clipboard, e não arquivo), o caminho acima de 259
  caracteres (o limite do descritor) e a lista grande demais para o canal local — essas aparecem no
  clipboard quando chegam, como sempre.

Dois detalhes do Win32 custaram rodadas: sem `Preferred DropEffect`, o Explorer lia a lista, criava
as pastas e não pedia conteúdo nenhum; e colar pelo `InvokeVerb` de outro processo não serve de
prova, porque a extração de arquivos virtuais precisa do processo do Explorer.

**Provado na bancada, no Explorer de verdade**: um arquivo de 3 MB chegando em três pedaços, um a
cada 2 s; Ctrl+V aos 2,0 s, com quase nada chegado; o Explorer terminou aos 6,3 s, logo depois do
último pedaço, com os 3 000 000 bytes certos — e a montagem foi renomeada (publicada) no meio da
leitura, sem conflito.

**No Linux não há promessa.** O `wl-copy` lê o conteúdo inteiro antes de assumir a seleção, e
prometer exigiria um cliente Wayland próprio. Pesa pouco: do Windows para o Linux a cópia começa no
Ctrl+C, porque o Windows avisa a mudança, e costuma ter chegado antes de a pessoa atravessar.

## Melhorias no caminho

- **O instalador do Windows encerra os ajudantes de antes.** O ajudante novo sai sozinho quando o
  executável muda, mas os que já rodam nas máquinas são de antes disso. O MSI agora os encerra
  (`taskkill`, silencioso) **depois** de parar o serviço e **antes** de trocar os arquivos — antes de
  parar, quem zela pelo ajudante o relançaria na hora com o executável antigo.
- **O envio do Linux percebe a rede sumida em meio minuto**, e não em ~15. As sondas do TCP só valem
  para a conexão parada; no meio de uma cópia há bloco no ar, e o Linux retransmitia por ~15 minutos
  antes de desistir — a retomada do log 55 só começava depois. `TCP_USER_TIMEOUT` de 30 s no Linux.
- **A publicação repete o `rename` negado por um instante**, por até três segundos (acima).
- **A janela não recebe mais o conteúdo do clipboard** (`Aviso::so_para_o_ajudante`): caminhos e
  textos que só o ajudante usa.

## Como foi verificado

- Windows e Fedora (container): suíte inteira e clippy limpos; `cargo xtask check` dentro das
  regras.
- Contra o canal inteiro (TCP, Noise e codec): quem recebe anuncia a chegada com os caminhos de
  verdade, e cada item está em `publicada_em/<caminho>` depois da publicação (`tests/chegada.rs`).
- Na bancada: o envio do Linux chegou ao clipboard do Windows depois de encerrado o ajudante velho;
  o ajudante novo saiu sozinho contra o serviço de antes; e a colagem antecipada no Explorer, acima.

## O que ainda depende do hardware

A volta inteira com os dois pacotes novos instalados: copiar uma pasta no Linux, atravessar, e colar
no Explorer antes de ela terminar de chegar. Instalar o MSI pede administrador.
