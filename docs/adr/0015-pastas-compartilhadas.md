# ADR-0015 — Pastas compartilhadas: origem, réplica e o ajudante que grava

**Status:** aceito · **Data:** 2026-10-02 · implementado: contrato ([log 57](../logs/57-a-pasta-compartilhada-o-contrato.md)), motor
([log 58](../logs/58-a-pasta-que-sincroniza.md)), sob demanda nos dois sistemas e telas
([log 59](../logs/59-a-pasta-sob-demanda.md)); falta a bancada Windows + Fedora

## O problema

Copiar e colar já leva arquivos de um computador ao outro ([log 34](../logs/34-copiar-aqui-colar-la.md)),
mas cada cópia é um ato: o arquivo chega, e daí em diante são dois arquivos sem relação. O pedido é
uma **pasta que existe nos dois computadores**, do jeito do OneDrive ou do Google Drive, para quem não
entende de rede:

- clicar em "Compartilhar uma pasta" (ou "Criar pasta compartilhada") num computador, e ela aparecer
  no outro, como uma pasta ou disco, **online ou offline**;
- no outro, ver a árvore inteira, mas baixar um arquivo **só quando ele for aberto**;
- uma edição fica salva onde foi feita e vai na hora ao outro lado, se ele estiver ao alcance; se não
  estiver, espera e vai sozinha quando a conexão voltar — com o IP que for;
- quando os dois lados mudam o mesmo arquivo ao mesmo tempo, fazer o que o mercado faz de melhor.

## A decisão

### 1. Cada pasta tem origem e réplica

Qualquer um dos dois computadores pode compartilhar. Quem compartilhou é a **origem** daquela pasta:
os arquivos de verdade estão lá, inteiros. O outro é a **réplica**. Os dois editam.

A origem é o **sequenciador**: cada mudança aceita — feita nela ou vinda da réplica — ganha o próximo
número da pasta, e esse número vira a versão da entrada. Com dois computadores isso é mais simples e
mais forte que um vetor de versões: há um só lugar que decide a ordem, e ele é justamente o que tem
os bytes.

### 2. Conflito: as duas versões ficam

A réplica manda, com cada mudança, a versão em que se baseou (`base`). Se a base ainda é a atual,
a mudança é aplicada. Se não é, **os dois lados mexeram no mesmo arquivo**, e:

- as duas versões ficam — nada se perde, que é o comportamento de Dropbox, OneDrive e Syncthing;
- a mais recente fica com o nome; a outra vira `nome (conflito NOTEBOOK 2026-10-02 14h30).ext`, ao
  lado da original;
- o relógio só escolhe o nome, e corrigido pela diferença entre as máquinas medida na abertura do
  canal: quem **descobre** o conflito é a versão, nunca o relógio, porque um notebook com a hora
  errada não pode decidir o que é conflito;
- empate fica com a origem, para os dois lados chegarem à mesma decisão sem conversar;
- a janela mostra um aviso com "Resolver": manter as duas, ficar com esta, ficar com a outra. A
  descartada vai para a lixeira da pasta, não some;
- **apagar contra editar: a edição vence.** A réplica apagou o que a origem tinha mudado — o arquivo
  volta. Apagar o trabalho de alguém sem perguntar é o pior desfecho possível;
- o que é apagado vai para uma lixeira de versões da pasta, com retenção.

### 3. Como a pasta aparece

| | Réplica no Windows | Réplica no Linux |
|---|---|---|
| Forma | **sob demanda**, pela Cloud Files API — a do OneDrive | **sob demanda**, por um sistema de arquivos em espaço de usuário (FUSE) |
| Onde | `%USERPROFILE%\InputRemote\<nome>`, com entrada própria no painel do Explorer | `~/InputRemote/<nome>`, com atalho na barra lateral do Nautilus |
| Ao abrir um arquivo não baixado | o Windows pede os bytes; o ajudante os busca na origem e os entrega à medida que chegam | ler só o começo traz só o trecho (a detecção de tipo do GNOME); ler além, ou até o fim, traz o arquivo inteiro para um cache escondido |
| Offline | a árvore inteira continua visível; abre o que já foi baixado ou fixado; o resto diz "rede indisponível" | o mesmo; o resto diz "rede inalcançável" |
| Sem a API | fora de NTFS: cópia completa | sem `fusermount3`: cópia completa |

No Linux, o conteúdo mora num cache na pasta de estado do usuário, e a pasta que a pessoa vê é a
montagem dele. Um arquivo que não veio é, no cache, um arquivo esparso com tamanho e data certos e
uma marca num atributo estendido; a marca vai junto num `rename` e some quando o download o
substitui. O que o ajudante muda na pasta visível — arquivo novo, renomeado, apagado — passa pela
montagem, para o núcleo avisar o gerenciador de arquivos na hora ([log 60](../logs/60-a-copia-e-a-pasta.md)). A montagem se declara `fuse.sshfs`, o único tipo FUSE que a GLib trata como remoto: o
Nautilus não abre cada foto para desenhar a miniatura. Gerador de miniaturas e indexador ouvem
"acesso negado" num arquivo que não veio.

Os ícones de nuvem e ✓ e o menu "Sempre manter neste dispositivo / Liberar espaço" vêm do próprio
Explorer quando a pasta é uma raiz de sincronia — não são desenhados por nós. A API funciona sem
pacote MSIX, inclusive a entrada no painel lateral pelo `StorageProviderSyncRootManager` — o *spike*
do [log 59](../logs/59-a-pasta-sob-demanda.md) provou. Uma raiz que ficou de uma instalação anterior
é tirada na partida do ajudante, e a desinstalação desliga as do usuário.

### 4. Quem grava é o ajudante, como o usuário

Um processo novo, `inputremote-agent --pastas`, roda **como o usuário**, na sessão dele — lançado e
vigiado pelo serviço no Windows, como o ajudante de clipboard; por uma unidade de usuário do systemd
no Linux. É ele que lê e grava a pasta, observa mudanças e fala com o par.

O serviço **não** abre a pasta:

- no Linux ele nem pode: `ProtectHome=read-only` na unidade;
- no Windows gravaria como `SYSTEM` na pasta do usuário, e qualquer erro de caminho viraria escrita
  privilegiada — o problema do *confused deputy* que o [log 34](../logs/34-copiar-aqui-colar-la.md)
  já teve de fechar para a cópia;
- a Cloud Files API registra a raiz de sincronia por usuário.

Processo **separado** do ajudante de clipboard, e não um modo dentro dele: o perfil de release usa
`panic = "abort"`, e um defeito na sincronia não pode levar o clipboard junto.

### 5. O serviço repassa, por um canal local próprio

O ajudante não tem a identidade Noise da máquina — ela é do serviço, e só dele. Então as mensagens da
pasta vão do ajudante ao serviço por um **terceiro canal local** (`inputremote-pastas`), e o serviço
as repassa ao par sem abri-las.

Canal próprio, e não o de controle: o de controle atende um pedido por vez pelo ator e descarta aviso
quando quem lê se atrasa. Bloco de arquivo não pode ser descartado nem esperar o ator. O repasse usa
fila limitada e contrapressão, fora do ator.

O serviço associa cada pasta ao usuário que a apresentou primeiro, e recusa outro usuário que a
reivindique: duas sessões abertas na mesma máquina não leem a pasta uma da outra.

### 6. No fio: canal 5, faixa própria, portão de versão

As mensagens viajam no canal 5 (TCP, Noise IK, o mesmo enlace e a mesma descoberta por identidade da
máquina que já sobrevivem a troca de IP e DHCP), dentro de uma variante nova, `BulkMessage::Folder`.
Protocolo **versão 8**.

- **Faixa própria.** A fila da cópia do clipboard faz uma cópia por vez, e a mais nova cancela a
  anterior. A sincronia não pode passar por ela: cancelaria o Ctrl+C do usuário, e seria cancelada
  por ele. As duas dividem o mesmo remetente, que já é compartilhado por `Mutex`.
- **Portão de versão.** Mensagem desconhecida derruba o enlace ([03, §8](../03-protocolo.md)). Uma
  mensagem da pasta mandada a um par da versão 7 derrubaria o canal 5 inteiro, e a cópia que
  estivesse passando com ele. `version::supports_folders` é a pergunta que quem envia faz antes.
- **Conteúdo por trecho.** A réplica pede `offset` e `len` (até 4 MiB) de uma entrada numa versão.
  Isso dá retomada de graça depois de uma queda — o que a cópia do clipboard não tem — e é exatamente
  a forma do pedido do Windows quando um arquivo sob demanda é aberto.
- **Controle de fluxo.** Quem pede guarda no máximo 4 MiB pendentes por pedido; o envio da réplica
  anda por crédito da origem. O canal 5 nunca fica ocupado a ponto de o clipboard esperar.
- **Arquivos nunca vão pelo rádio**, como na cópia. Só com Bluetooth ao alcance, a pasta espera a
  rede, e a frase diz isso.

### 6a. A cópia (Ctrl+C) e a pasta não mandam o mesmo conteúdo duas vezes

- Copiar de dentro de uma pasta compartilhada leva os caminhos, pela pasta (`Copied`), e não os
  bytes: o outro computador põe no clipboard os arquivos da cópia dele.
- O que atravessou pela cópia — o que foi e o que chegou — fica anotado numa lista do usuário. Quando
  a pasta precisa de um conteúdo, procura nela antes de pedir à rede, pelo tamanho e pelo BLAKE3.
- Detalhes e a prova em bancada no [log 60](../logs/60-a-copia-e-a-pasta.md).

### 7. O que nunca sincroniza

Os temporários de editor (`~$*.docx`, `.~lock.*#`, `.swp`, `4913`, `*.crdownload`), os arquivos que o
sistema escreve sozinho (`Thumbs.db`, `desktop.ini`, `.DS_Store`) e a pasta de controle `.inputremote`.
Sincronizar o arquivo de trava do Word travaria o documento no outro computador.

## Alternativas rejeitadas

| Alternativa | Por que não |
|---|---|
| Compartilhamento de rede do sistema (SMB/Samba) | Exige entender de rede — usuário, senha, firewall, IP —, não funciona offline, e não há Samba por padrão no Fedora |
| Sincronia de espelho nos dois lados, sem origem (modelo Syncthing) | Disco dobrado e nada sob demanda; com dois computadores, um vetor de versões resolve o que o sequenciador resolve com menos estado |
| Segunda conexão TCP só para a pasta | Ninguém aceita conexão enquanto o canal 5 está de pé, e um byte de propósito antes do Noise quebraria o aperto de mão da versão 7. Fica como plano B se a bancada mostrar o clipboard esperando |
| Identidade Noise no ajudante | A chave da máquina sairia do serviço para um processo do usuário |
| Serviço gravando a pasta | `ProtectHome` no Linux; escrita como `SYSTEM` no Windows |
| Índice em SQLite | Uma dependência nativa a mais para o que é um mapa de caminhos; retrato em `postcard` mais diário basta até o limite de 100 mil entradas |
| Último que salvou ganha | Some com trabalho sem avisar |

## Consequências

- **Fases**, cada uma entregável:
  1. contrato — este ADR, o protocolo 8, o vocabulário do canal local e o crate `ir-pasta`;
  2. motor e espelho completo nos dois sistemas, que prova sincronia, conflito, offline e reconexão;
  3. sob demanda no Windows;
  4. telas e o Explorer;
  5. sob demanda no Linux.
- **Crates novos:**
  - `ir-pasta`, puro, para as decisões;
  - `ir-sincronia`, com E/S, no ajudante;
  - `ir-nuvem`, a pasta sob demanda: a Cloud Files API no Windows, FUSE no Linux;
  - `ir-acervo`, o disco de uma pasta (varredura, montagem, lixeira, índice guardado, trechos),
    separado do `ir-sincronia` quando ele passou do teto de linhas.

  Os dois últimos entram com as fases deles, cada dependência nova com o ADR dela (`notify`, o
  seletor de pasta, as *features* de Cloud Files do `windows`).
- Um par na versão 7 continua conversando em tudo o que já fazia; a pasta só não aparece, e a janela
  diz "atualize o InputRemote no outro computador".
