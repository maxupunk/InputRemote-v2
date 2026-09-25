# Uma regra em um lugar: a revisão de duplicidade

**Data:** 2026-09-24

**Itens:** [09](../09-padroes-de-codigo.md) — padrões de código; [02, §2](../02-arquitetura.md) — as
setas de dependência; [04, §3.1](../04-seguranca.md) — a impressão digital.

**O que foi pedido:** rever o código atrás de duplicidade de código e de fluxo, e refatorar tudo com
DRY e SOLID, mantendo o funcionamento.

## O que a revisão achou

Cinco revisões em paralelo (sessão e protocolo; serviço e agente; entrada e transporte; interface e
arquivos; e entre crates) acharam perto de setenta repetições. O que decidiu a ordem foi que **nove
delas já tinham divergido** — a mesma regra escrita duas vezes, e as duas cópias dando respostas
diferentes. Esses defeitos foram corrigidos primeiro, cada um com um teste que falhava antes.

## Defeitos que eram duplicidade

- **Esquecer o par não devolvia a política de Ctrl+Alt+Del do Windows**, nem contava ao agente, nem
  reavaliava a tela protegida. Os passos da permissão estavam em quatro lugares; agora são um,
  `permissao_do_protegido_mudou` (`actor/protegido.rs`), e a decisão "a política fica ligada?" é a
  função pura `permitido_em`.
- **Parear mudava a configuração em memória antes de gravar**, e trocava o destino dos arquivos
  descartando o endereço de rede já achado. Passou pelo caminho único de gravação (`persistir_com`).
- **"Limpar agora" apagava a cópia que ainda estava chegando**: a faxina não conhecia o nome das
  montagens (`.parcial-*`). A regra do nome é uma só (`ir_files::staging::e_montagem`), e as órfãs de
  um serviço morto saem na subida (`recolher_orfas`).
- **Quem enviava e quem recebia davam nomes diferentes à mesma cópia** ("b.txt e outros" contra
  "a.txt e outros"), e os avisos de erro iam sem nome. Um nome só: `nome_da_entrega`.
- **Uma queda no meio de um envio avisava duas vezes**, e uma queda sem cópia em curso mostrava "a
  cópia não atravessou". `anunciar_queda` só avisa se havia cópia.
- **O mesmo erro de Bluetooth dizia "pareie" no Linux e "não atendeu" no Windows.** Cada sistema só
  classifica o código cru (`FalhaDeConexao`); a mensagem sai de um lugar (`para_erro`).
- **A sessão recriada não sabia da economia do Wi-Fi** por até 30 s: a subida e a recriação
  alimentavam a sessão nova cada uma do seu jeito. Agora é `alimentar_sessao_nova`.
- **"Par suspenso" aparecia como normal e mesmo assim notificava "Conexão perdida".** A gravidade de
  cada queda é uma função exaustiva, `MotivoDaQueda::gravidade`.
- **Trocar de portador com a sessão de pé não zerava a área de transferência** de um lado, e o outro
  zerava. Abrir e fechar uma encarnação da sessão são duas funções (`open_incarnation`,
  `reset_incarnation_state`), em vez de quatro listas de campos.
- **A rolagem fina do touchpad do Windows se perdia no Linux** (a divisão por 120 descartava o
  resto): `AcumuladorDeRoda`, o mesmo do touchpad.
- **A posição do ponteiro virava pixel de quatro jeitos**, e os injetores ignoravam o monitor. Uma
  conversão só, em `ir-geometry` (`Desktop::to_virtual_fraction`, `from_position`). Com um monitor
  nada muda; com vários, o ponteiro cai no monitor certo — **falta provar em bancada com dois
  monitores**.
- **O `reg.exe` rodava sem prazo dentro do laço do serviço.** Agora roda numa thread própria, com
  prazo.
- **O repassador do rádio tardio só repassava a primeira mensagem**: um rádio perdido e reaberto duas
  vezes não chegava ao ator.
- **O canal de pareamento pela rede engolia a falha ao mandar a confirmação**; o do Bluetooth a
  contava. Os dois contam.

## O que virou um lugar só

- **A camada segura** de rede e Bluetooth era escrita duas vezes. Agora é `ir_crypto::enlace`, pura:
  envelope, remontagem de quadros, contador implícito, fecho do aperto de mão e a confirmação do
  pareamento. O Bluetooth passou a recusar o estranho **antes** do aperto de mão, como a rede.
- **Os tipos de entrada**: injeção e captura estavam em três formas cada, com seis traduções que
  descartavam em silêncio o que não conheciam. Agora são `ir_proto::input::{Injection, Capture}`, sem
  `_ =>`: um evento novo é erro de compilação em cada ponta.
- **A tela protegida** era decidida por texto em quatro lugares, com regras de maiúscula diferentes.
  O agente decide uma vez (`ir_input::desktop::protegido`) e conta o resultado.
- **A impressão digital** tinha duas formas para a mesma máquina: Base32 do BLAKE3 no registro e
  hexadecimal do identificador na janela. As duas são agora a de [04, §3.1](../04-seguranca.md).
- **`ir-processo`**, crate novo e sem nada do produto: o registro (antes copiado no serviço e no
  agente) e a ferramenta do sistema com prazo (antes em `ir-energia` e `ir-sessao`).
- **O endereço do canal local** era resolvido em três lugares (`ir_ipc::endereco`); a leitura e a
  escrita de quadros bloqueantes em quatro (`ir_ipc::codec::{ler_de, escrever_em}`); a porta padrão
  em três (`ir_proto::DEFAULT_PORT`); e um IP sem porta passou a valer a porta padrão também no
  transporte, como na janela.
- **No serviço**: um `avisar_estado` no lugar de 21 cópias, um `encerrar_sessao` no lugar de 5, a
  gravação da configuração por dois caminhos (`persistir_com`, `gravar_ja`), a borda e o portador
  fixado lidos da configuração em vez de copiados, e o `Zelador` cuidando do agente como já cuidava do
  ajudante de clipboard.
- **Na sessão**: devolver o controle, parar de receber, injetar tecla e botão e mandar quadro fora de
  sequência têm um caminho cada. Os portões de versão que `MIN_SUPPORTED` já tornava verdadeiros
  saíram.

## Mudanças de comportamento

- Perder o agente enquanto recebe devolve o controle com `Reclaim`, como todo outro caminho.
- Um `Hello` que chegue pelo TCP de arquivos é ignorado.
- A janela mostra um aviso por vez, com prioridade decidida em `Estado::aviso_principal`.
- A falha de lançar o agente é registrada uma vez por motivo, e não a cada tentativa.

## Como foi verificado

- Windows: 1 180 testes, clippy `-D warnings` e `cargo xtask check` limpos.
- Container do Fedora: 1 127 testes, clippy limpo.
- O protocolo no fio não mudou: os vetores e os testes de fio passam sem alteração. O canal entre
  serviço e agente mudou de forma — os dois são instalados juntos.

## Na bancada

O RPM novo foi instalado no Fedora, contra o Windows ainda com o serviço de antes (sem
administrador aqui, o MSI não pôde ser instalado):

- **A camada segura refeita conversa com a antiga.** O enlace subiu pelo Bluetooth, a rede entrou
  na rota dupla, e o canal de arquivos se estabeleceu — os três apertos de mão de `ir_crypto::enlace`
  contra o código que não mudou.
- **O Fedora, bloqueado, controla o Windows.** Com um mouse USB de mentira (`uinput` com outro nome,
  que a captura trata como físico), o ponteiro atravessou a borda direita e voltou pela borda.
- **E achou mais uma regra aplicada pela metade.** Rolar a roda no Windows, controlado pelo Fedora,
  devolvia o controle ao Windows ("retomado sem atravessar"). O gancho de mouse ignorava só o
  **movimento** injetado; o clique e a roda que o próprio serviço injetava voltavam como entrada
  local, e a sessão retomava. O gancho de teclado já ignorava toda injeção. Agora o de mouse também,
  num lugar só, antes de classificar o evento (`windows/hooks.rs`). A correção está no MSI novo.

Depois, com o MSI novo instalado no Windows (código novo dos dois lados):

- **A rolagem não devolve mais o controle.** Com o Windows ocioso havia 47 s (`GetLastInputInfo`,
  para nenhuma mão no mouse contaminar o teste), o Fedora atravessou, rolou três marcações no
  Windows, e voltou pela borda — sem nenhuma retomada.
- **O Windows controla o Fedora bloqueado.** Com alguém usando o mouse do Windows, o controle passou
  para o Fedora e voltou, com a tela do Fedora na tela de bloqueio — o que o log 53 queria.
- Um teste feito com gente mexendo no mouse do Windows mostrou retomadas a ~170 ms de cada entrada:
  é a regra funcionando (mexer aqui retoma, passada a carência de 150 ms), e não defeito. O teste
  que vale é o com a máquina ociosa.

- **Copiar e colar não atravessava para o Windows desde 22/09, e o motivo não era esta revisão.** O
  ajudante de clipboard do Windows era um processo de **23/09**, que sobreviveu a todas as
  atualizações: falava o canal de antes com o serviço novo, cada aviso vinha "quadro inválido", ele
  reconectava para sempre — e, com a trava de instância única, impedia o novo de subir. Agora o
  ajudante que não entende o serviço **sai**, e quem zela por ele lança o instalado
  (`clipboard::e_incompativel`); uma queda de canal continua sendo reconexão. Com o processo velho
  encerrado, o texto `teste-log54-224907` saiu do Fedora e chegou ao Windows, e uma imagem do
  Windows chegou ao Fedora.

## O que ainda depende do hardware

O ponteiro no monitor certo com **dois** monitores: o Windows da bancada tem um só (3440×1440), e
com um a travessia foi provada acima. A conversão está coberta pelos testes de `ir-geometry`.
