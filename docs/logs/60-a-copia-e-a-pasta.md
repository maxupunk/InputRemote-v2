# A cópia e a pasta, sem o mesmo arquivo atravessar duas vezes; e o Nautilus que só via com F5

**Data:** 2026-10-03

**Itens:** [ADR-0015](../adr/0015-pastas-compartilhadas.md) — pastas compartilhadas;
[log 59](59-a-pasta-sob-demanda.md) — a pasta sob demanda; [log 34](34-copiar-aqui-colar-la.md) — a
cópia de arquivos; [PROGRESSO](../../PROGRESSO.md), Etapa 11.

**O que foi pedido:** com a pasta compartilhada em uso, duas melhorias.

1. **A cópia (Ctrl+C) e a pasta brigavam pela rede.**
   - O que se copia de dentro da pasta compartilhada não deve ir pela cópia de arquivos: já está
     nos dois computadores.
   - E o que foi copiado num computador, chegou ao outro e foi colado na pasta compartilhada de lá,
     não deve atravessar de novo pela sincronia. Ela deve aproveitar o que já veio.
   - Duas cópias do mesmo conteúdo na rede ao mesmo tempo disputam o enlace e podem travar as duas.
2. **No Linux, depois de sincronizar, o gerenciador de arquivos só mostrava a mudança com F5.** No
   Windows ela aparecia na hora.

## O que mudou

### Ctrl+C de dentro da pasta: vão os caminhos, e não os bytes

O ajudante de clipboard agora conhece as pastas compartilhadas deste computador. Ele pede a lista ao
se ligar ao serviço e a atualiza a cada `Aviso::PastasMudaram`.

Quando a pessoa copia arquivos de dentro de uma pasta, o ajudante não pede `EnviarArquivos`. O
caminho é outro:

| Onde | O que acontece |
|---|---|
| Ajudante de clipboard | Pede `Pedido::Pasta(ComandoDePasta::Copiado { pasta, caminhos })`, com os caminhos relativos |
| Ajudante das pastas | Leva ao outro computador uma mensagem nova do protocolo, `FolderMessage::Copied`, com os caminhos |
| Ajudante das pastas do outro lado | Traduz cada caminho para a cópia **dele** da pasta (só os que existem lá) e os devolve ao serviço (`DoAjudanteDePastas::PorNoClipboard`) |
| Serviço | Repassa ao ajudante de clipboard como `Aviso::ArquivosDaPasta` |
| Ajudante de clipboard do outro lado | Publica os caminhos no clipboard |

Colar lá cola o arquivo da própria pasta. Se ele ainda não veio (sob demanda), vem pela pasta, uma
vez só.

Algumas regras:

- Os caminhos são conferidos dos dois lados (`is_safe_relative_path`).
- Uma cópia de mais de 1 000 arquivos, ou que não caiba num quadro, não põe nada no clipboard de lá.
- Numa cópia misturada (de dentro e de fora da pasta), só os de fora vão pela cópia de arquivos. Os
  de dentro nunca vão.

### O que veio pela cópia e é colado na pasta: achado aqui, pelo conteúdo

O ajudante de clipboard anota numa lista, por usuário, na pasta de estado dele
(`ir-acervo/src/conhecidos.rs`):

- os arquivos que mandou;
- os que chegaram pela cópia.

Quando a pasta precisa de um conteúdo, o ajudante das pastas procura antes nessa lista. Procura um
arquivo do mesmo tamanho cujo BLAKE3 é o mesmo: a conta da cópia e a da pasta são a mesma, sobre os
mesmos bytes. O resumo de cada candidato é calculado uma vez e guardado pelo caminho, tamanho e
horário. A lista guarda os 500 mais recentes, por 24 horas.

| Quem precisa | Onde | O que faz com o conteúdo achado |
|---|---|---|
| A origem, num envio da réplica | `viva/origem.rs` | Copia daqui e responde `AlreadyHave`: os blocos não atravessam |
| A réplica, antes de um download | `Baixas::comecar` | Copia daqui, conferindo o resumo enquanto copia |
| O pedido sob demanda (Windows e Linux) | `Viva::buscar` | Serve o trecho do arquivo daqui; funciona até offline |

A cópia sempre confere o resumo enquanto copia: o original pode ter mudado depois de conferido.

### Linux: o que muda na pasta passa pela montagem

**A causa.** O ajudante gravava direto no cache, por baixo da montagem FUSE. O núcleo não via essas
mudanças e não avisava o inotify, que é por onde o Nautilus (o `GFileMonitor` da GLib) fica sabendo.
No Windows o ajudante grava na própria pasta, e o Explorer vê.

**A correção.** Na réplica sob demanda do Linux, criar, mover, criar subpasta e levar à lixeira
agora passam pela montagem (`Viva::pela_montagem`). Ler continua sendo no cache.

- **A pasta de controle.** O conteúdo é montado em `.inputremote/montagem`. Essa pasta passou a
  existir, pela montagem, só para o próprio processo do ajudante, conferido pelo `Tgid` de quem pede
  (`Sistema::entrada`). Para os outros processos e na listagem, ela continua não existindo.
- **A lixeira.** Ela mora na pasta de estado, fora da montagem. O caminho sai da vista pela montagem
  (para `.inputremote/montagem`) e de lá vai à lixeira direto. Um `rename` da montagem até a lixeira
  cruzaria sistemas de arquivos.

## O defeito que a correção quase criou, e o que a bancada achou

1. **Um trava-tudo em potencial.** Antes, a montagem tinha 4 threads, e um programa esperando um
   download prendia uma delas até o conteúdo chegar. Com o ajudante mexendo na pasta pela montagem,
   quatro programas esperando a rede deixariam o próprio ajudante sem quem o atendesse. E o ajudante
   é quem traz o conteúdo que eles esperam.

   Agora a leitura, a escrita e o encurtar que precisam esperar respondem de uma thread própria, e as
   threads do FUSE ficam livres. O FUSE permite responder de outra thread, e `Sistema` passou a ser
   barato de clonar.

   O teste `com_programas_esperando_conteudo_o_ajudante_ainda_mexe_na_pasta` põe seis leitores
   esperando e cria e move pela montagem em menos de 2 s.
2. **"Invalid cross-device link" ao apagar**, achado no notebook: a primeira versão levava o arquivo
   da montagem direto à lixeira, que está fora dela.

   Pior, a remoção falhada deixou o arquivo no notebook. A varredura seguinte o tratou como novo, e
   ele voltou ao Windows. Corrigido com os dois passos acima; depois disso, criar e apagar não deram
   mais aviso de erro.

## A bancada

Este Windows (instância de teste, origem) e o notebook Fedora 44 com GNOME 50 (instância de teste,
réplica sob demanda, ao lado do pacote instalado e da pasta `temp` que o usuário já usa, sem tocá-los).

**Linux, o gerenciador de arquivos.** A pasta estava aberta no Nautilus, e um `gio monitor` (o mesmo
`GFileMonitor` que o Nautilus usa) a observava. Sem F5, os eventos vieram:

| No Windows | O que o monitor do notebook viu |
|---|---|
| Arquivo novo | `moved in` |
| Subpasta nova | `created` |
| Renomear `notas.txt` | `renamed to notas-renomeadas.txt` |
| Apagar um arquivo e uma subpasta | `moved out`, os dois |

**A cópia e a pasta:**

| Passo | Resultado |
|---|---|
| Ctrl+C de `fotos/img2.jpg` dentro da pasta, no Windows | O clipboard de verdade do notebook (Wayland) passou a ter `file://` de `~/InputRemote/Bancada/fotos/img2.jpg`, a cópia do notebook. `img2.jpg` continuou sem conteúdo lá, e o cache em 0: nenhum byte atravessou |
| Uma imagem de 2 MB veio ao notebook pela cópia e foi colada na pasta de lá | Chegou ao Windows igual em 2,3 s. O registro do Windows: "o conteúdo já estava neste computador; não atravessou a rede" |
| A mesma imagem de 3 MB já estava no notebook (veio pela cópia), e o Windows a colou na pasta | Ler o começo foi servido do arquivo do notebook; ler inteiro a copiou de lá ("já estava neste computador"); o resumo é igual ao do Windows, e o Windows não serviu nada |

## Arquivos

- `crates/ir-proto/`:
  - `src/message/pasta.rs`: `FolderMessage::Copied`, no fim
  - `src/message/pasta/validacao.rs`
  - `src/limits.rs`: `MAX_COPIED_PATHS`
  - `tests/vectors/pasta.rs`: vetor `folder_copied`; `main.rs`: 23 variantes
- `crates/ir-ipc/`:
  - `src/pastas.rs`: `ComandoDePasta::Copiado`, `DoAjudanteDePastas::PorNoClipboard`
  - `src/ui.rs`: `Aviso::ArquivosDaPasta`, só para o ajudante
  - testes de número no fio; `examples/pastas.rs`: `copiar` e `clipboard`, para a bancada
- `crates/ir-canais/src/pastas.rs`: repassa `PorNoClipboard`.
- `crates/ir-acervo/`:
  - `src/conhecidos.rs` (novo): a lista, a procura pelo resumo, a cópia conferida e o servir do arquivo daqui
  - `src/baixa.rs`, `src/disco.rs` (`guardar_na_lixeira`)
- `crates/ir-sincronia/`:
  - `src/pastas/copia.rs` (novo), `src/pastas.rs`, `src/pastas/comandos.rs`, `src/laco.rs`
  - `src/viva.rs`, `src/viva/{origem,replica,nuvem}.rs`, `src/viva/nuvem/sistema.rs` (`pela_montagem`, `levar_a_lixeira`)
  - testes: `tests/comum/mod.rs` (a bancada dividida), `tests/copia_e_pasta.rs` (novo), `tests/dois_ajudantes.rs`
- `crates/ir-nuvem/src/linux/`:
  - `sistema.rs`: `Sistema` clonável, a pasta de controle para o próprio processo
  - `fs.rs`: as respostas que esperam, noutra thread
  - `testes.rs`: inotify e os seis leitores
- `crates/ir-agent/src/clipboard/`: `pasta.rs` (novo), `mod.rs`, `servico.rs`, `testes.rs`.

## Verificação

- **Windows:**
  - `cargo xtask check`, `cargo clippy --workspace --all-targets -D warnings` e `cargo fmt` limpos;
  - `cargo test --workspace`: 1 325 passando, nenhuma falha.
- **Linux, num Fedora 44 em contêiner com `/dev/fuse`:**
  - `clippy -D warnings` limpo;
  - 1 341 testes passando, nenhuma falha, entre eles os 12 da montagem e o de ponta a ponta.
- **Os testes novos:**
  - `copia_e_pasta.rs`: copiar de dentro põe no clipboard do outro os arquivos dele; colar na pasta o
    que a origem já tinha não manda blocos; colar na origem o que a réplica já recebeu não baixa
    trechos. Sem a lista de conhecidos, os dois últimos falham, com 5 blocos e 5 trechos
    atravessando; com ela, zero.
  - O que a montagem publica gera evento inotify.
  - Com seis leitores esperando a rede, o ajudante ainda mexe na pasta.
  - Copiar de dentro da pasta, no laço do ajudante de clipboard, vira `Pedido::Pasta` e não
    `EnviarArquivos`.
  - O protocolo grava o vetor de `Copied`.
- A bancada acima, com as duas máquinas de verdade.

## Decisões

- **A lista de conhecidos é um arquivo do usuário, e não uma conversa com o serviço.** Os dois
  ajudantes rodam como o mesmo usuário. O caminho e o resumo de um arquivo de um usuário nunca passam
  pelo serviço, nem chegam ao ajudante de outro usuário. E o serviço não mudou para isso.
- **Achar pelo conteúdo, e não pelo nome.** A pessoa pode colar com outro nome, noutra subpasta, ou
  colar outro arquivo com o mesmo nome. Tamanho e BLAKE3 iguais são o mesmo conteúdo; o resto, não.
- **Copiar de dentro da pasta não manda nada quando o outro computador está offline.** O clipboard
  de lá não muda; o conteúdo, de todo modo, não atravessaria.
- **Reiniciar o ajudante remonta a pasta.** Quem a observava (um Nautilus aberto nela) observa a
  montagem antiga, e precisa reabrir a pasta. É o mesmo que acontece com um pendrive tirado e
  posto de novo.
