# A pasta sob demanda nos dois sistemas, e as telas que resolvem o conflito

**Data:** 2026-10-02

**Itens:** [ADR-0015](../adr/0015-pastas-compartilhadas.md) — pastas compartilhadas;
[ADR-0016](../adr/0016-dependencias-da-pasta-compartilhada.md) — as dependências;
[log 58](58-a-pasta-que-sincroniza.md) — o motor; [PROGRESSO](../../PROGRESSO.md), Etapa 11, F2 a F4.

**O que foi pedido:** terminar a pasta compartilhada. No outro computador a pasta aparece inteira,
online ou offline, e um arquivo só é baixado quando é aberto, como no Google Drive. Esta entrada
cobre o "sob demanda" no Windows (F2) e no Linux (F4), as telas (F3) e a divisão do ajudante, que
passou do teto de linhas.

## O que existe agora

**Windows: a Cloud Files API, a mesma do OneDrive (`ir-nuvem/src/windows/`).**

- **A raiz.** A pasta recebida vira uma raiz de sincronia em `%USERPROFILE%\InputRemote\<nome>`, com
  entrada própria no painel lateral do Explorer, registrada por `StorageProviderSyncRootManager`. Se
  o registro do painel falhar, ela cai para `CfRegisterSyncRoot`: a pasta funciona sem a entrada.
- **Os marcadores.** Cada arquivo da origem aparece com nome, tamanho e data certos, sem os bytes.
  Uma mudança da origem atualiza o marcador (`CfUpdatePlaceholder`) em vez de baixar.
- **Abrir.** O Windows chama o provedor (FETCH_DATA). O ajudante pede o trecho à origem, até 4 MiB
  por vez, e entrega cada pedaço que chega (`CfExecute`, TRANSFER_DATA).
- **Offline.** Sem o outro computador, o pedido é recusado na hora com
  `STATUS_CLOUD_FILE_NETWORK_UNAVAILABLE`. O programa ouve "rede indisponível" em vez de esperar o
  prazo de 60 s.
- **O menu do Explorer.** "Sempre manter neste dispositivo" traz o conteúdo; "Liberar espaço" o tira
  do disco, mas só de um arquivo sem mudança pendente. Os ícones de nuvem e ✓ são do próprio
  Explorer. Depois de uma mudança daqui chegar à origem, o arquivo é marcado em dia
  (`CfSetInSyncState`).
- **Parar de compartilhar a réplica.** O que já veio vira arquivo comum (`CfRevertPlaceholder`); o
  que não veio sai do disco, porque está inteiro na origem. A raiz sai do Windows.
- **Fora de NTFS** (um pendrive FAT, por exemplo) a réplica é cópia inteira, como na F1.

**Linux: um sistema de arquivos em espaço de usuário (`ir-nuvem/src/linux/`, `fuser`).**

- **O cache e a montagem.** A réplica guarda o conteúdo num cache na pasta de estado do usuário
  (`~/.local/state/inputremote/pastas/<id>/conteudo`). A pasta que a pessoa vê, `~/InputRemote/<nome>`,
  é a montagem desse cache, feita como o usuário pelo `fusermount3`.
- **Um arquivo que não veio.** No cache, é um arquivo esparso com tamanho e data certos e uma marca
  no atributo estendido `user.inputremote.sem_conteudo`. A marca vai junto num `rename`, e o download
  que substitui o arquivo a apaga sem ninguém precisar lembrar.
- **Abrir** um desses arquivos não traz nada. **Ler só o começo** de um arquivo maior (até 256 KiB,
  sem chegar ao fim) traz só aquele trecho da origem, como no Windows, e o arquivo continua sem
  conteúdo — é o que a detecção de tipo do GNOME faz. **Ler além do começo, ou até o fim**, põe o
  arquivo na frente da fila de downloads e quem leu espera ele chegar inteiro; daí em diante ele está
  no disco e abre sem rede. A montagem roda com 4 threads: quem espera não trava a pasta para os
  outros.
- **Offline**, abrir para ler um arquivo que não veio falha na hora com `ENETUNREACH` ("rede
  inalcançável"). Gravar por cima (`>`, o "salvar" de quase todo programa) funciona offline:
  esvaziar não precisa do conteúdo antigo.
- **Miniaturas e indexação.** A montagem se declara `fuse.sshfs` na tabela de montagens — o único
  tipo FUSE que a GLib chama de remoto —, e o Nautilus, com a preferência padrão "miniaturas só de
  arquivos locais", não abre cada foto para desenhar o ícone. Por garantia, gerador de miniaturas
  (qualquer processo com "thum" no nome) e indexador (`tracker-…`, `localsearch-…`) recebem `EACCES`
  num arquivo que não veio.
- **Gravar** pela montagem — criar, editar, renomear, apagar — vira a mesma operação no cache. A
  varredura de sempre a vê e manda à origem.
- **Parar de compartilhar** traz para a pasta visível, como arquivo comum, o que já veio; o resto fica
  para trás.
- **Sem `fusermount3`**, a réplica é cópia inteira. O pacote passa a pedir `fuse3`.

**As telas (F3).**

- **Progresso.** A linha de cada pasta diz "Recebendo 3 arquivos…" e "Enviando 2 mudanças…", e,
  offline, quantas mudanças esperam o outro computador.
- **Conflito.** Cada conflito aparece com a frase de quem é a cópia e três botões: **Manter as
  duas**, **Só a mais recente**, **Só a de NOTEBOOK**. A versão que não fica vai para a lixeira da
  pasta, recuperável por 30 dias, e a tela diz isso. "Ver as duas" abre a pasta.
- **Lixeira.** O botão abre a lixeira da pasta. Restaurar é arrastar de volta: o arquivo reaparece
  como uma mudança comum e sincroniza.
- **Oferta.** Quando o outro computador compartilha uma pasta, o aviso chega mesmo com a janela
  fechada. No Windows é o aviso no canto da tela, o mesmo das cópias, da interface que mora na
  bandeja; no Linux, o `notify-send` do próprio ajudante.

## O *spike* do painel do Explorer sem pacote

O risco aberto do ADR-0015 era a entrada no painel lateral sem identidade de pacote MSIX. Ele caiu:

- **O registro funciona sem pacote.** `StorageProviderSyncRootManager::Register` grava a raiz em
  `HKLM\…\SyncRootManager` e a entrada do painel em `HKCU\…\Desktop\NameSpace`; a pasta aparece no
  painel com o nome "Projetos (InputRemote)" e o ícone da janela.
- **`GetSyncRootInformationForId` falha para uma raiz escondida em `AppData`.** As raízes de verdade
  ficam em `%USERPROFILE%\InputRemote`, onde funciona; o teste de registro usa o mesmo lugar.
- **`GetCurrentSyncRoots` volta vazio sem pacote.** A lista das raízes registradas, necessária para
  limpar as órfãs, é lida do próprio registro (`SyncRootManager`, pelo prefixo
  `InputRemote!<usuário>!`).
- **Uma raiz que sobra de uma execução anterior, no mesmo caminho, faz o registro novo responder
  "Acesso negado".** O ajudante agora tira, na partida, toda raiz deste usuário que não pertence a
  nenhuma pasta que ele conhece.
- **Na desinstalação**, a raiz é por usuário e o instalador roda como SYSTEM. Uma ação no
  `Produto.wxs` roda, como o usuário, `inputremote-agent --pastas --desregistrar`. Esse comando
  desliga as pastas sob demanda deste usuário, com o mesmo destino de "parar de compartilhar": o que
  veio fica, o que não veio sai. A ação não roda numa atualização.

## Os defeitos que a prova de ponta a ponta achou

Duas instâncias inteiras nesta máquina, como no log 58, agora com a réplica sob demanda:

1. **O portão de versão piscava com a sessão de entrada.** A faixa da pasta só abre para um par da
   versão 8 ou mais. O serviço sabia a versão do par só enquanto a sessão de entrada estava de pé; a
   cada queda dela, a pasta via o par como "desatualizado" e parava. Agora a versão fica guardada:
   - ao vivo, num valor que só muda quando há par;
   - em disco, no arquivo `versao-do-par`, com a chave do par, para valer depois de reiniciar o
     serviço.
2. **"Atualize o InputRemote no outro computador" aparecia antes de saber a versão.** Logo depois de
   ligar, a versão ainda era desconhecida, e desconhecida contava como velha. O enlace agora diz se a
   versão é conhecida (`versao_conhecida`); até lá, a pasta só espera.
3. **A réplica sob demanda num teste sem provedor ficava sem conteúdo para sempre.** Sem quem atenda
   o Windows, um marcador nunca vira arquivo. A réplica só é sob demanda quando há provedor ligado
   (`repassar`); sem ele, é cópia inteira.
4. **Linux, achado na montagem de verdade: o tamanho velho cortava a leitura.** O núcleo guarda os
   atributos por 1 s. Se o conteúdo chega com outro tamanho que o do marcador (a origem mudou no
   meio), quem abriu lia só até o tamanho antigo. Agora, quando o conteúdo chega, o ajudante avisa o
   núcleo para esquecer os atributos daquele arquivo (`inval_inode`).
5. **A pasta de 10 mil arquivos dizia "em dia" com 1 528.** As mudanças de uma pasta grande vêm em
   várias mensagens. Entre uma e a seguinte, a réplica sob demanda não tinha nada pendente (marcador
   não baixa), e dizia "Em dia nos dois computadores" com 15% da árvore no disco. Agora a réplica
   sabe que uma leva pedida, ou começada, ainda não terminou (`mudancas_por_vir`), e diz
   "Sincronizando…" até a última mensagem. O teste `enquanto_a_leva_de_mudancas_nao_termina_a_pasta_nao_diz_em_dia`
   falha sem a correção e passa com ela.

E os que só o notebook de verdade, com o GNOME 50, mostrou:

6. **O Nautilus baixava todas as fotos de uma pasta só por olhá-la.** Primeiro parecia o gerador de
   miniaturas, e a lista de nomes estava velha: no Fedora 44 eles são `glycin-thumbnailer`,
   `papers-thumbnailer`, `gst-*-thumbnailer`; a lista tinha `gdk-pixbuf`, `evince`, `totem`. Virou
   regra (qualquer nome com "thum"), e não bastou. O registro de quem abriu — agora o ajudante anota
   o **processo** de cada download, nunca o nome do arquivo — mostrou o próprio `nautilus`, numa
   thread `pool-N` da GLib. O FUSE entrega o número da thread, e `/proc/<tid>/comm` é o nome dela; o
   processo vem do `Tgid`. Mesmo com as miniaturas desligadas, o Nautilus continuou abrindo os
   `.png`, e não abria os `.txt`: no Fedora 44, `*.png` casa com dois tipos (`image/png`, peso 50, e
   `image/apng`, peso 40), e a GLib lê o começo de cada arquivo para decidir. Como abrir trazia o
   arquivo inteiro, olhar uma pasta de 200 fotos baixaria todas. Agora abrir não traz nada, e ler o
   começo traz só o trecho (`ir-nuvem/src/linux/trechos.rs`); e a montagem se declara `fuse.sshfs`,
   para a GLib a ver como remota.
7. **Um arquivo pequeno lido inteiro por trechos nunca ficava no disco.** Cada leitura voltava à rede,
   e offline ele não abria. Ler até o fim agora traz o arquivo inteiro: num arquivo pequeno são os
   mesmos bytes, e ele fica.
8. **Trocar o ajudante com o Nautilus aberto na pasta deixava a pasta morta** ("Transport endpoint is
   not connected") até reiniciar. A desmontagem comum da montagem que sobrou dizia "ocupado", e o
   ajudante novo não montava. Agora ele desmonta a que sobrou de forma preguiçosa
   (`fusermount3 -u -z`), antes de olhar o ponto.
9. **Offline, ler um arquivo que não veio dizia "Input/output error".** A leitura passa pelo cache de
   páginas do núcleo, que troca o motivo. Agora o ajudante diz à montagem quando o outro computador
   está ao alcance (`Montagem::alcance`), e sem ele abrir para ler falha na hora com "Network is
   unreachable".
10. **Offline, gravar por cima de um arquivo nunca aberto falhava**: esvaziá-lo pedia o conteúdo
    antigo primeiro. Esvaziar não precisa dele; só encurtar para outro tamanho precisa.

Nenhum desses cinco aparecia no contêiner: lá não há Nautilus, nem `*.png` ambíguo, nem um GNOME
lendo a pasta. Cada um ganhou teste da montagem (`ir-nuvem/src/linux/testes.rs`) ou dos nomes de
processo, e os testes passam no contêiner com `/dev/fuse`.

| Passo, Windows sob demanda | Resultado |
|---|---|
| B aceita `Projetos` (com um arquivo de 9 MiB) | A árvore aparece na hora, só com marcadores, e a pasta está no painel do Explorer |
| Outro programa lê o arquivo de 9 MiB | O conteúdo vem por trechos em cerca de 5 s; o resumo é igual ao da origem |
| O serviço de A cai; B abre um arquivo que não veio | Falha em 226 ms com "rede indisponível"; o que já tinha vindo abre normalmente |
| O serviço de A volta | O mesmo arquivo abre, sem ninguém fazer nada |
| B para de compartilhar | O que veio ficou como arquivo comum, o que não veio saiu, e a raiz saiu do painel; A esqueceu a pasta |
| O ajudante reinicia com uma raiz órfã registrada | A órfã sai, e a pasta registra de novo |

## A divisão do ajudante: `ir-acervo`

O `ir-sincronia` passou do teto de 2 500 linhas de produção ([09, §1](../09-padroes-de-codigo.md)).
O que saiu foi o que já não conhecia o laço, a sessão com o par nem o serviço, só o motor e o disco:

- a varredura;
- a montagem ao lado, a publicação e a lixeira;
- o índice guardado;
- os trechos que vêm e vão;
- o *trait* `Saida`.

Tudo isso foi para um crate novo, `ir-acervo`. O `ir-sincronia` o reexporta com os mesmos nomes, então
nenhum caminho mudou por dentro. Também foram divididos:

- `viva/nuvem.rs` (598 linhas): a cola com cada sistema foi para `viva/nuvem/sistema.rs`;
- `ir-nuvem/src/linux/fs.rs` (458 linhas): o `Sistema` e o que ele sabe do cache foram para
  `linux/sistema.rs`.

## Arquivos

- `crates/ir-nuvem/` (novo):
  - `src/lib.rs`
  - `src/windows/{mod,registro,conexao,marcador,testes}.rs`
  - `src/linux/{mod,sistema,fs,inodes,trechos,testes}.rs`
- `crates/ir-acervo/` (novo): `src/{lib,varredura,disco,guardado,baixa,envio}.rs`, que vieram do
  `ir-sincronia`.
- `crates/ir-sincronia/`:
  - `src/{lib,laco,pastas,viva,atalho}.rs`
  - `src/pastas/comandos.rs`
  - `src/viva/{nuvem,origem,replica}.rs`, `src/viva/nuvem/sistema.rs`
  - `tests/sob_demanda_linux.rs`
- `crates/ir-ipc/src/pastas.rs`, com `EscolhaDeConflito`, `ConflitoDePasta`, `baixando` e
  `ComandoDePasta::{Resolver, AbrirLixeira}`, e `src/pastas/frases.rs`.
- `crates/ir-transferencia/src/desvio.rs`, com `versao_conhecida`.
- `crates/ir-daemon/src/actor/pastas.rs` e `src/arquivos.rs`, com a versão guardada.
- `crates/ir-agent/src/main.rs`, com `--pastas --desregistrar`.
- A janela:
  - `crates/ir-ui/ui/{pastas,pastas-tipos,dados}.slint`
  - `crates/ir-ui/src/janela/pastas.rs`, `src/simulado.rs`
- Empacotamento:
  - `empacotar/windows/Produto.wxs`, com a ação `DesligarPastas`
  - `empacotar/linux/inputremote.spec`, com `Requires: fuse3`
- Raiz e regras: `Cargo.toml`, `xtask/src/deps.rs`.
- Docs:
  - ADR-0015 (status, §3), ADR-0016 (`fuser`, `libc`, *features* do `windows`, `windows-future`)
  - `docs/02-arquitetura.md`, `USAR.md`

## Verificação

- **Windows:**
  - `cargo xtask check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt` e
    `cargo test --workspace` limpos: 1 318 testes passando, 11 ignorados.
  - Os 4 ignorados do `ir-nuvem`, rodados à mão nesta máquina, passam:
    - registro e painel do Explorer;
    - marcador lido por outro programa com o provedor entregando os bytes;
    - provedor que falha sem travar quem abre;
    - raízes lidas do registro.
- **Linux, num Fedora 44 em contêiner com `/dev/fuse`:**
  - `cargo clippy --workspace --all-targets -D warnings` limpo;
  - `cargo test --workspace`: 1 330 testes passando, nenhuma falha;
  - os 7 testes da montagem do `ir-nuvem`: ler o começo traz só o trecho; ler além, ou até o fim,
    traz o arquivo inteiro, mesmo com outro tamanho; sem o par, ler dá `ENETUNREACH` na hora;
    offline, gravar por cima funciona; escrever, criar e renomear chegam ao cache;
  - o teste novo de ponta a ponta, `tests/sob_demanda_linux.rs`: dois ajudantes de verdade, a réplica
    montada. A árvore aparece com os tamanhos antes de qualquer conteúdo, um arquivo de 3 MB vem por
    trechos quando é aberto, offline abrir falha com `ENETUNREACH`, o que foi gravado offline pela
    montagem chega à origem quando o enlace volta, e o arquivo abre de novo.
  - Cada um rodou três vezes seguidas, sem falha.
- A prova de ponta a ponta no Windows, na tabela acima, foi refeita depois da divisão em
  `ir-acervo`, com os binários novos:
  - o arquivo de 9 MiB veio por trechos em 5,4 s (*build* de depuração), com o resumo igual;
  - sem o serviço de A, abrir um arquivo que não veio falhou em 26 ms com "indisponibilidade da
    rede";
  - com A de volta, o mesmo arquivo abriu, e o que B gravou offline chegou a A;
  - `inputremote-agent --pastas --desregistrar` deixou o que veio como arquivo comum, sem marcador.
- **Pasta grande, Defender e o "salvar" do Office**, nas duas instâncias desta máquina, com os
  binários de depuração:

  | Passo | Resultado |
  |---|---|
  | A compartilha 10 000 arquivos em 100 subpastas (105 MB); B aceita | Os 10 000 marcadores aparecem em 22,5 s; a tela diz "Sincronizando…" até o último, e "Em dia" aos 25 s |
  | O Defender varre a raiz de B (`MpCmdRun -Scan -ScanType 3`), com a proteção em tempo real ligada desde o começo | 0,5 s, nenhuma ameaça, e os 10 000 continuam sem conteúdo: nada foi baixado |
  | A edita 100 arquivos de uma subpasta | Os 100 marcadores de B mudam em 5 s; abrir um traz a versão nova |
  | B edita um arquivo que nunca baixou | Chega a A em 4,5 s |
  | O "salvar" do Word dentro da raiz de B, num `.docx` nunca baixado: trava `~$relatorio.docx`, temporário `~WRL0001.tmp`, original renomeado para `~WRD0000.tmp`, temporário renomeado para o nome, `~WRD` apagado | A recebe o conteúdo novo; nenhum temporário, trava ou cópia de conflito aparece em A; os dois dizem "Em dia" |

  O Word não está instalado aqui: a sequência de arquivos é a dele, feita à mão.
- **A bancada Windows + Fedora**, com o notebook de verdade (Fedora 44, GNOME 50, Nautilus 50.3) e
  uma segunda instância lá, como o usuário, ao lado do pacote instalado e sem tocá-lo, pareada pela
  rede com a instância de teste deste Windows:

  | Passo | Resultado |
  |---|---|
  | O Windows compartilha `Bancada`; o aviso chega ao notebook; ele aceita | A pasta aparece montada em `~/InputRemote/Bancada`, com a árvore e os tamanhos certos; o cache ocupa 0 bytes; o atalho entra na barra lateral |
  | O Nautilus abre uma pasta com três fotos de 5 MB, e outra com fotos e um arquivo de 2 MiB | Nada é baixado; o cache não cresce |
  | `cp` do arquivo de 2 MiB, e a leitura inteira de uma foto | Vêm inteiros, com o resumo igual ao do Windows (0,7 s para os 2 MiB) |
  | O serviço do Windows cai | A tela diz "O outro computador não está ao alcance"; o que veio abre; o que não veio falha em 4 ms com "Network is unreachable" |
  | Offline, o notebook grava por cima de `notas.txt` (nunca aberto) e cria um arquivo; depois o Windows edita `notas.txt` | A tela do notebook diz "1 mudança esperando" |
  | O serviço do Windows volta | Sem ninguém fazer nada: `notas.txt` com a versão do Windows (a mais recente), `notas (conflito fedora 2026-10-03 07h23).txt` com a do notebook, o arquivo novo nos dois; os dois dizem "Em dia", com um conflito |
  | O ajudante do notebook é trocado com o Nautilus aberto na pasta | A pasta volta a funcionar sozinha (defeito 8) |
  | O notebook para de compartilhar | A montagem sai, o que veio fica como arquivo comum, o que não veio não fica, o atalho sai; o Windows esquece a pasta |

  Depois, o notebook ficou como estava: a instância de teste, `~/ir-e2e` e `~/InputRemote` foram
  apagados, e o serviço instalado continuou ativo.
- **A entrada durante uma sincronia grande**, na mesma bancada, pelo Wi-Fi. O que se mede é o atraso
  de ida e volta da sessão de entrada entre os dois serviços, o mesmo do diagnóstico, em janelas de
  10 s com cerca de 49 amostras cada. A latência de verdade, da captura à injeção, exige o serviço
  instalado nas duas máquinas; este é o *proxy* de transporte.

  | | Mediana | Pior caso típico | Picos |
  |---|---|---|---|
  | Parado (60 s) | 2 ms | 4 a 9 ms | 49 e 103 ms |
  | A réplica no notebook baixando 1 GB da pasta (255 s, ~4 MB/s; resumo igual nos dois lados) | 3 a 4 ms | 15 a 24 ms | 123, 129 e 193 ms, em 3 de 25 janelas |
  | A cópia de arquivo que já existia (canal do clipboard), 256 MB | 2 a 3 ms | 7 a 20 ms | — |

  Dentro da meta do produto para UDP em rede local ([01, §6](../01-visao-e-escopo.md): mediana abaixo de
  8 ms, p99 abaixo de 25 ms), menos os picos, que o Wi-Fi também dá parado. Fora da meta relativa da
  Etapa 8 (no máximo 10% de piora): a mediana vai de 2 para 3 a 4 ms. Mas a cópia que já existia
  custa o mesmo, e a entrada viaja em UDP, fora da conexão TCP da pasta: quem enche é o rádio, que as
  duas dividem. Por isso a segunda porta TCP que o plano previa como saída não mudaria nada; o que
  mudaria é a sincronia ceder enquanto alguém usa o teclado e o mouse do outro, e isso só se mede com
  o serviço instalado nas duas máquinas.
- **O instalador, com uma pasta sob demanda registrada no perfil de verdade**
  (`%USERPROFILE%\InputRemote\Desinstalar`), rodado como administrador pelo usuário: instalar
  o MSI novo por cima do anterior, desinstalar, reinstalar. O registro do ajudante (horário local):

  | Quando | O que o ajudante registrou |
  |---|---|
  | 08:17:42, depois de instalar por cima | "pastas compartilhadas carregadas pastas=1", "pasta sob demanda registrada no Explorer": a atualização manteve a pasta |
  | 08:22:00, na desinstalação | "pastas sob demanda desligadas do Windows quantas=1": a ação `DesligarPastas` rodou como o usuário |
  | 08:22:25, depois de reinstalar | "pastas=0": nada sobrou |

  Depois da desinstalação, a raiz não estava mais no registro. O efeito nos arquivos — o que veio
  fica como arquivo comum, o que não veio sai — já tinha sido provado com o mesmo comando, mais
  acima. O instantâneo de antes não serviu para isso: a raiz de teste tinha sido tirada antes, pela
  limpeza de órfãs de uma instância de bancada que roda como o mesmo usuário do Windows (a
  bancada tem dois "computadores" num usuário só; no uso real há um ajudante por usuário). O
  MSI também mostrou um defeito do próprio `Produto.wxs`: o comentário novo tinha `--`, que o XML
  não aceita, e o `candle` recusava o arquivo — o CI falharia.
- **Um editor de verdade, o LibreOffice 26.2.6**, extraído do MSI oficial sem instalar
  (`msiexec /a`) e conduzido pela própria API dele (UNO), como uma pessoa: abrir, acrescentar uma
  frase, salvar por cima, fechar.

  | Passo | Resultado |
  |---|---|
  | B abre `relatorio.docx`, que nunca tinha baixado, edita e salva | Abriu em 2,0 s (o conteúdo veio sob demanda); a trava `.~lock.relatorio.docx#` existiu durante a edição; A recebeu o documento com o texto original e a edição; nenhuma trava, temporário ou cópia de conflito apareceu em A; os dois dizem "Em dia" |
  | A edita o mesmo documento e salva | B o recebe com as três partes; os dois dizem "Em dia" |

  O Word não está instalado aqui. A sequência de arquivos dele foi feita à mão, mais acima.
- **Não verificado ainda:**
  - o Word de verdade (não instalado nesta máquina);

## Decisões

- **No Linux, só o começo de um arquivo é servido por trecho; o resto vem inteiro.** O cache continua
  sendo sempre um arquivo inteiro e comum, e a varredura de sempre serve para ele; o trecho do começo
  fica só na memória de quem leu. Servir qualquer parte por trecho exigiria um cache por bloco e um
  mapa do que veio, para ganhar pouco: quem lê além do começo quase sempre lê o arquivo todo.
- **A montagem se diz `fuse.sshfs`.** A pasta é de rede, e entre os FUSE a GLib só reconhece isso
  por esse nome. O problema é antigo e conhecido
  ([nautilus#1209](https://gitlab.gnome.org/GNOME/nautilus/-/issues/1209)): o rclone e o onedriver
  sofrem o mesmo. A fonte continua `inputremote`, que é o que o `findmnt` mostra. De quebra, o
  `updatedb` não indexa a pasta.
- **Miniatura e indexador recebem "acesso negado"**, e não um ícone genérico. É o que o núcleo deixa
  dizer sem mentir sobre o conteúdo; o Nautilus desenha o ícone do tipo.
- **A versão do par fica guardada em disco**, com a chave dele: um par trocado não herda a versão do
  anterior.
- **Uma raiz sobrando do Windows é tirada sem perguntar.** Ela só pode ser deste produto (o prefixo
  tem o usuário) e de uma pasta que este ajudante não conhece mais. Deixá-la quebra o registro da
  pasta nova no mesmo caminho.
- **Fica uma chave vazia no HKLM** (`SyncRootManager\InputRemote!<usuário>!…`) de uma raiz desta
  bancada, sem caminho. É inofensiva: o Explorer a ignora, e a limpeza só a vê quando há caminho.
