# A pasta que sincroniza: o motor, o ajudante e a primeira prova de ponta a ponta

**Data:** 2026-10-02

**Itens:** [ADR-0015](../adr/0015-pastas-compartilhadas.md) — pastas compartilhadas;
[ADR-0016](../adr/0016-dependencias-da-pasta-compartilhada.md) — as dependências;
[log 57](57-a-pasta-compartilhada-o-contrato.md) — o contrato; [PROGRESSO](../../PROGRESSO.md),
Etapa 11, F1.

**O que foi pedido:** continuar a pasta compartilhada até o fim. Esta entrada é a F1: a sincronia
funcionando nos dois sistemas, com a réplica baixando tudo — o "sob demanda" do Windows é a F2.

## O que existe agora

**O motor (`ir-pasta`, puro).**

- **A origem** (`origem.rs`, `origem/decisao.rs`):
  - numera cada mudança;
  - decide o que fazer com cada envio, remoção e subpasta criada na réplica;
  - num conflito, guarda as duas versões, e a mais recente fica com o nome;
  - apagar contra editar: a edição vence.
- **A réplica** (`replica.rs`, `replica/varrida.rs`, `replica/mudancas.rs`):
  - transforma a varredura do disco numa fila de mudanças, guardada para valer offline;
  - aplica as levas da origem sem sobrescrever o que mudou aqui;
  - antes de baixar, procura o conteúdo no disco daqui: o arquivo que a origem renomeou é movido, e
    a versão daqui que perdeu um conflito é levada para a cópia de conflito em vez de baixada de novo.

**O ajudante (`ir-sincronia`), um processo próprio, como o usuário: `inputremote-agent --pastas`.**

- **Disco:** varre a pasta, recebe aviso do sistema quando ela muda (`notify`) e varre depois de 1,5 s
  de silêncio.
- **Gravação:** monta ao lado em `.inputremote/montagem` e publica com `rename`. O que sai vai para
  uma lixeira com retenção de 30 dias.
- **Transferência:** baixa por trechos de 4 MiB e manda com crédito da origem.
- **Índice:** guardado em disco com troca atômica.
- **Atalho:** põe a pasta recebida na barra lateral do Nautilus.

**O serviço** só repassa:

- a faixa da pasta no canal 5 (`ir-transferencia/src/desvio.rs`), com o portão da versão 8 e a
  conferência de cada mensagem que chega;
- o canal local das pastas (`ir-canais/src/pastas.rs`), que entrega cada mensagem ao ajudante do
  usuário dono da pasta — o `uid` no Linux, o SID no Windows;
- no Windows, o zelador que sobe o ajudante na sessão (`zelar_pelas_pastas`); no Linux, a unidade
  `inputremote-pastas.service`.

**A janela.**

- Na tela inicial, um cartão com uma linha sobre as pastas, que chama para a oferta quando há uma.
- A tela "Pastas compartilhadas":
  - para cada pasta: a situação, Abrir e Parar (segurar);
  - Aceitar e "Agora não" para as ofertas;
  - "Compartilhar uma pasta deste computador…" e "Criar";
  - a faixa de conflito, que diz que as duas versões foram guardadas.
- O seletor de pasta é o nativo: `rfd` no Windows, `zenity` no Linux.

## Os defeitos que a simulação achou

A bancada de dois nós (`ir-pasta/tests/simulacao`) roda o motor de verdade com dois discos em memória
e um canal que liga e desliga. As histórias sorteadas fecharam com 20 mil sementes, edições e
remoções dos dois lados. Antes disso, acharam três defeitos que nenhum teste escrito à mão achava:

1. **A versão da origem que não voltava depois de um conflito.** Quando a réplica perdia o conflito, a
   versão da origem tinha chegado numa leva anterior, com o caminho sujo, e foi guardada sem tocar o
   disco. O conteúdo daqui ia para a cópia de conflito e o caminho ficava vazio para sempre. Agora quem
   desocupa um caminho traz o que a origem tem nele.
2. **A remoção que sumia da fila.** Apagado, recriado e apagado de novo, offline, o arquivo continuava
   na origem: a segunda remoção tirava da fila tudo daquele caminho, inclusive a primeira, que ainda
   valia. Agora só sai da fila o que mandaria conteúdo.
3. **O nome de cópia de conflito reaproveitado.** Uma cópia de conflito de antes, apagada nos dois
   lados, deixou o nome livre para a seguinte; a remoção pendente da antiga segurou a nova como
   "suja", e o conteúdo daqui nunca foi para ela. Agora, quando a origem devolve algo que a réplica
   apagou ("ressuscitado"), a réplica reavalia o caminho com o mesmo reaproveitamento de uma leva.

E um quarto, de desenho, achado ao escrever o laço: o `Hello` da pasta precisava dizer **se é
resposta**. Sem isso, um ajudante que reinicia enquanto o outro já o tinha como presente nunca era
respondido. A versão 8 não foi lançada, então o vetor gravado `folder_hello` mudou no lugar (um byte,
o `reply`).

## A prova de ponta a ponta nesta máquina

Duas instâncias inteiras do produto lado a lado. Cada uma tinha o próprio serviço, ajudante, porta,
estado e "pasta pessoal". O pareamento foi Noise de verdade, com o código de seis dígitos, sobre TCP
de verdade. Sequência:

| Passo | Resultado |
|---|---|
| A compartilha `Projetos` (3 arquivos, um de 12 MiB) | B vê a oferta: "O outro computador quer compartilhar esta pasta com este." |
| B aceita | A pasta aparece em `B\perfil\InputRemote\Projetos`, com os três arquivos iguais por resumo; os dois dizem "Em dia nos dois computadores." |
| B edita `notas.txt` e cria `novos/criado-no-b.txt` | Os dois aparecem em A em segundos |
| O serviço de B cai | A diz "O outro computador não está ao alcance." |
| Offline, A e B editam `notas.txt`; B cria `offline-b.txt`; A apaga `relatório/março.txt` | — |
| O serviço de B volta | Sem ninguém fazer nada, as árvores ficam **idênticas**: `notas.txt` com a versão de B, a mais recente; `notas (conflito E2E-A 2026-10-02 22h03).txt` com a de A; o arquivo novo de B nos dois; `março.txt` fora dos dois. Os dois contam um conflito. |

## Arquivos

- `crates/ir-pasta/`:
  - `src/{acao,retrato,origem,replica}.rs`
  - `src/origem/{decisao,testes}.rs`, `src/replica/{mudancas,varrida,testes}.rs`
  - `tests/simulacao/{main,bancada,aleatorio}.rs`
- `crates/ir-sincronia/` (novo):
  - `src/{lib,laco,pastas,viva,baixa,envio,disco,varredura,vigia,guardado,lugar,atalho}.rs`
  - `src/pastas/comandos.rs`, `src/viva/{origem,replica}.rs`
  - `tests/dois_ajudantes.rs`
- Protocolo e canal local:
  - `crates/ir-proto/src/message/pasta.rs`: `reply` no `Hello`, `FolderMessage::folder`
  - `crates/ir-proto/src/message/pasta/motivos.rs`
  - `crates/ir-ipc/src/pastas.rs`: `Recado`, `MensagemDoPar`; `src/pastas/frases.rs`
  - `crates/ir-ipc/src/ui.rs`: `Pedido::Pastas`, `Pedido::Pasta`, `Resposta::Pastas`,
    `Aviso::PastasMudaram`, `Aviso::RecadoDasPastas`; `src/falha.rs`
  - `crates/ir-ipc/examples/pastas.rs`
- Serviço:
  - `crates/ir-transferencia/src/desvio.rs` (com ganchos em `lib.rs`, `sessao.rs`, `recebendo.rs`,
    `pedidos.rs`)
  - `crates/ir-canais/src/{pastas,escuta,lib}.rs`
  - `crates/ir-acesso/src/identidade.rs` (`sid`)
  - `crates/ir-sessao/src/{lancador,zelador,lib}.rs`
  - `crates/ir-daemon/src/{main,arquivos}.rs`, `src/actor/{mod,partes,pedidos,pastas,bancada}.rs`
- Ajudante e janela:
  - `crates/ir-agent/src/{main,pastas}.rs`, `src/clipboard/{mod,atualizacao,instancia}.rs`
  - `crates/ir-ui/ui/{pastas,pastas-tipos,dados,app,inicio}.slint`
  - `crates/ir-ui/src/{escolher_pasta,lib,janela,simulado}.rs`, `src/janela/pastas.rs`
- Empacotamento: `empacotar/linux/{inputremote-pastas.service,ajudante-nas-sessoes,inputremote.spec}`
- Raiz: `Cargo.toml`, `xtask/src/deps.rs`
- Docs: `docs/adr/0016-dependencias-da-pasta-compartilhada.md`, `docs/00-indice.md`

## Verificação

- `ir-pasta`, 28 testes de unidade e 14 de simulação:
  - as histórias da réplica e da origem, uma a uma;
  - o índice que sobrevive a ser guardado e lido;
  - 300 histórias sorteadas em cada execução; rodadas à parte com 5 mil e 20 mil sementes (histórias
    mais longas, com remoção de subpastas inteiras) fecharam sem divergência e sem perda.
- `ir-sincronia`, 7 testes de unidade e 5 de integração com dois ajudantes de verdade em pastas
  temporárias:
  - um arquivo de 9 MiB que passa por vários trechos e créditos;
  - conflito offline;
  - lixeira; parar deixa os arquivos;
  - recusa de pasta dentro de outra compartilhada;
  - ajudante que reinicia e continua do índice guardado.
- A prova de ponta a ponta acima, com os binários de verdade.
- `cargo xtask check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt`,
  `cargo test --workspace`: limpos no Windows; 1 314 testes passando, 7 ignorados (os de hardware).
- **Não verificado ainda:** a bancada Windows + Fedora (o Linux não foi compilado nesta rodada), e
  a tela das pastas aberta à mão. O modo `--simulado` da janela mostra três pastas de demonstração
  para isso.

## Decisões

- **Uma operação da réplica em voo por vez.** Garante a ordem que a origem precisa (subpasta antes do
  conteúdo, conteúdo antes da subpasta apagada) sem número de sequência a mais. O custo é uma ida e
  volta por arquivo pequeno; a medir na bancada.
- **O identificador de cada pasta pertence ao primeiro usuário que fala dela**, em memória no serviço.
  Reiniciar o serviço refaz a associação com quem conectar primeiro; guardar em disco fica para
  quando houver mais de um usuário com pastas na mesma máquina.
- **A réplica não renomeia na origem nesta versão**: manda remoção e criação, e a origem reconhece o
  conteúdo pelo resumo (`AlreadyHave`), sem receber os bytes de novo. A mensagem `Rename` existe no
  protocolo para a F2.
- **Parar de compartilhar não apaga nada**, dos dois lados. A pasta vira uma pasta comum.
