# Os processos e a cópia no painel

**Data:** 2026-10-24 · **Branch:** `feat/painel-android`

**Itens:** [12, §3.2, §3.3, §5.2](../12-painel-android.md); [ADR-0018](../adr/0018-painel-no-android.md);
[PROGRESSO](../../PROGRESSO.md), Etapa 12.

**O que foi pedido:** "o card de cópia de arquivos em tempo real, com atualização igual ao do
processador e memória"; "a opção de monitorar um processo, exemplo Docker ou WSL"; "o botão de 'i'
na CPU e na memória que abre a lista de processos com CPU e memória, com atualização em tempo real".

## O que mudou

### Entre os computadores: o protocolo 11 virou um recado opaco

A versão 11 nasceu ([log 68](68-o-painel-no-android.md)) com `Control::WatchUsage` e
`Control::Usage(UsageSample)`. Pedir processos ao par exigiria mexer nela de novo, e de novo a cada
coisa que o painel quisesse mostrar. Então ela virou **`Control::Panel(Vec<u8>)`**: bytes que a
sessão não lê, até `MAX_PANEL_MESSAGE` = 440 (cabe num quadro do rádio), só para um par que negociou
a 11 (`version::supports_panel`). O que vai dentro é do `ir-monitor` (`EntrePcs`, com vetores
gravados): `Assinar(InteresseEmProcessos)`, `Parar` e `Uso(UsageSample, Option<Processos>)`. Uma
lista que não cabe encolhe até caber; a amostra e os vigiados vão sempre.

Exceção de pré-lançamento ([`crates/ir-proto/tests/vectors/main.rs`](../../crates/ir-proto/tests/vectors/main.rs)):
a 11 nunca saiu desta branch, então os vetores dela foram trocados no lugar (`hello_v11`, `panel`).

### Entre o computador e o painel: a versão 2 do vocabulário

Duas mensagens no fim (`ir_monitor::mensagem`, `VERSAO = 2`): `DoPainel::Interesse` (os processos dos
cards e de qual computador a lista está aberta) e `ParaPainel::Detalhes` (a cópia e os processos de
cada computador). O computador só manda detalhes a quem disse interesse, e o aplicativo só diz
interesse a um computador que se apresentou na versão 2 — dizer a um da versão 1 derrubaria a
ligação, porque ele não conhece a mensagem.

O quadro e os detalhes de um segundo saem juntos; o aplicativo segura o quadro até os detalhes
chegarem (`ir-monitor-android/src/montagem.rs`), mas no máximo um segundo: se não vierem, o quadro
sai sem eles, e a tela não congela.

### O que se mede, e quando

- **`ir-telemetria/src/processos.rs`**: a parte pura. O sistema dá uma `Foto` (cada processo com o
  tempo de processador desde que nasceu, e o total da máquina); o uso é a diferença entre duas
  fotos dividida pelo tempo da máquina, a mesma conta do processador da amostra. Agrupado pelo nome:
  é como a pessoa pensa ("o Chrome está pesado"), e o grupo cabe no recado do rádio.
- **Um card de processo soma tudo que contém o nome**, sem caixa: `wsl` pega `wslhost`,
  `wslservice` e `VmmemWSL`; `docker`, o Docker Desktop e o `com.docker.backend`.
- **A lista**: os cinco mais pesados em processador somados aos cinco mais pesados em memória.
- **Só com interesse**: a vigia mede processos quando um card de processo existe ou a lista está
  aberta, aqui ou no painel do par. Sem nada disso, nenhuma foto. A medida é uma só para os dois
  painéis, e cada um recebe a parte que pediu (`Processos::para`).
- **Windows**: a memória é o *working set* privado, a coluna "Memória" do Gerenciador de Tarefas —
  somar o *working set* inteiro contaria duas vezes as páginas que os processos do Chrome dividem.
  **Linux**: o `rss` de `/proc/<pid>/stat`, que inclui as compartilhadas.

### A cópia

A vigilância ouve o mesmo `Aviso::Transferencia` que a janela recebe e mede a velocidade com o mesmo
`Velocimetro`, que saiu da `ir-ui` para a `ir-ipc`: as duas telas dizem a mesma velocidade da mesma
cópia. A cópia fica no painel enquanto anda e 8 s depois de acabar.

### O aplicativo

- **O "i"** no processador e na memória de cada computador abre a lista daquele computador, já
  ordenada pela pergunta feita: pelo processador num, pela memória no outro. A lista ocupa quase a
  tela, atualiza a cada segundo, e cada linha tem **+ Acompanhar**.
- **Cards de processo** (`proc:docker`) são cards como os outros no `Arranjo`, até quatro. Eles são
  o pedido ao computador: o que está no arranjo é o que ele mede. Por isso não entram sozinhos (o
  card de outro painel no mesmo computador não aparece neste) e, tirados, são esquecidos. Em
  **Editar painel → Acompanhar processo**, um nome ou os atalhos `docker`, `wsl` e `vmmem`.
- **A faixa da cópia**: o que vai, a porcentagem, a velocidade e quanto falta. Deitado, divide a
  linha com a faixa do enlace; na edição, mostra o lugar, para a pessoa poder tirá-la sem esperar
  uma cópia.
- O `Controlador` guarda o interesse e o diz a cada ligação nova; `Tela.Painel.lista` diz qual lista
  está aberta, e sobrevive a girar a tela.

## O que foi provado

- **Rust**: os testes dos crates tocados passam no Windows e no Fedora 44 (contêiner); `clippy` sem
  aviso nos dois; `cargo xtask check` em dia. Novos: a soma e a lista dos processos, a medida entre
  fotos, a vigia que só mede com interesse, o interesse juntado por computador, os detalhes que só
  vão a quem pediu (TCP de verdade), o recado entre os computadores e a cópia com velocidade na
  vigilância, a montagem do quadro com os detalhes, e o aplicativo de ponta a ponta recebendo a cópia.
- **Kotlin**: 30 testes de JVM, 11 deles novos (o arranjo com processos, a leitura da lista e da
  cópia, a ordem da lista e a ordem que fica parada, o interesse no controlador).
- **No tablet de verdade** (M8_4G), contra a bancada, que agora simula uma cópia a cada minuto e os
  processos do par: o "i" abre a lista ao vivo, ordenada pela pergunta; **+ Acompanhar** vira card nos
  dois computadores; `dartvm` passou a "não está rodando" quando acabou; `docker` somou 12
  processos; a faixa da cópia andou, mostrou a velocidade e o tempo, e sumiu depois.
- O arranjo de cards de quem usa o tablet foi guardado antes do teste e devolvido idêntico depois, e
  os pareamentos de teste com a bancada foram tirados do aparelho.

## O que deu errado no caminho

- **A foto dos processos era lenta.** A primeira versão abria cada processo (`OpenProcess`,
  `GetProcessTimes`, `K32GetProcessMemoryInfo`): com ~400 processos e a máquina ocupada, de 0,3 a
  1,8 s por foto. A amostra do painel saía a cada 3 s, e o tablet caiu por silêncio. Trocada por
  **uma** chamada, `NtQuerySystemInformation(SystemProcessInformation)`, a do Gerenciador de
  Tarefas: de 6 a 9 ms na mesma máquina, ainda ocupada (um pico de 112 ms com a máquina a 100 %).
  Os campos de tempo não têm nome no `windows-sys`; os deslocamentos são os do `phnt`, estáveis
  desde o Windows 7, conferidos contra os limites dos bytes antes de ler.
- **A lista andava debaixo do dedo.** Reordenada a cada segundo, o toque em "+ Acompanhar" do Code
  caiu no `dartvm`, que tinha subido para o lugar dele. Agora os valores mudam a cada segundo, mas as
  linhas só trocam de lugar a cada 5 s (`ListaEstavel`), ou na hora quando a pessoa escolhe outra
  coluna.
- **"+ Acompanhar" saía como "+"**: a largura do botão não cabia o texto.
- **Com as duas faixas empilhadas, deitado**, os cartões dos computadores perdiam um terço da altura.
  Passaram a dividir a linha; e o detalhe da cópia põe a velocidade antes do nome, para quem perder
  letras num card estreito ser o nome.
- **Um teste de ponta a ponta esperava o quadro que não vinha**: o painel diz o interesse depois do
  olá do computador, e o computador só mandaria os detalhes no segundo seguinte. Agora o interesse é
  respondido na hora com o quadro e os detalhes — o que também faz a lista abrir sem esperar.
- O teste do monitor perdia a segunda mensagem quando as duas chegavam numa leitura só; ganhou um
  leitor de várias mensagens.

## O que falta

- A bancada com o serviço **instalado** nos dois computadores (Etapa 12): agora também os processos
  do par de verdade e uma cópia de verdade.
- O orçamento de [12, §3.4](../12-painel-android.md) com a lista aberta.
