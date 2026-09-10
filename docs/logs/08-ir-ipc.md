# `ir-ipc`: o contrato que a interface enxerga

**Data:** 2026-09-09

**Itens:** 1.3 — `ir-ipc` (protocolo de controle), canal do agente separado, autoridade por
pedido, vocabulário próprio do contrato.

**O que foi feito.** O crate que fica entre o serviço e a interface. Três decisões o definem.

**O estado é publicado, não vazado.** `status::Estado` não é `ir_session::Phase` exposto: é um
tipo próprio, com frases prontas para a tela, que o serviço preenche traduzindo. Essa tradução é
a fronteira. No v1 ela não existia, e o crate da interface acabou com 10.491 linhas — mais que
transporte e plataforma somados.

**Cada pedido declara a autoridade que exige.** `Pedido::autoridade()` devolve `Ler`,
`Configurar` ou `Elevado`, e o transporte consulta antes de entregar. A autorização deixa de ser
uma sequência de `if`s espalhados pelo serviço e passa a ser uma propriedade do pedido. O teste
`todo_pedido_declara_autoridade` existe porque um pedido que esqueça de se classificar é escalada
de privilégio, não descuido de estilo: quem fala com este serviço consegue digitar a senha de
administrador.

**`Injetar` não existe no vocabulário da interface.** São dois canais, `ui` e `agent`, com
transportes e descritores de segurança distintos. A ausência do verbo é a garantia: se qualquer
processo do usuário pudesse pedir injeção, qualquer programa que ele rodasse poderia digitar no
prompt de UAC, e o modelo de segurança do Windows na máquina cairia junto.

**Toda falha diz o que fazer.** `Falha::o_que_fazer()` é obrigatório por teste, com mínimo de
tamanho para não aceitar "tente de novo" disfarçado. `CodigosDiferentes` tem um teste próprio
exigindo que a frase mencione o risco de alguém no meio — dizer "tente de novo" ali ensinaria o
usuário a insistir exatamente onde ele não deve.

**Correções de contrato feitas no caminho.** Três campos e um pedido faltavam, e cada um foi
descoberto pela interface não conseguir desenhar algo:

- `Estado::portador_fixado`, separado de `Estado::portador`. Um é o que está valendo, o outro é a
  preferência gravada. Sem os dois, quem fixou Bluetooth e está na rede vê "rede" e acha que a
  preferência foi ignorada sem explicação.
- `Estado::bloqueio_permitido`, separado de `nivel_privilegiado`. Um diz se a máquina *consegue*,
  o outro se o usuário *deixou*. `impedimento()` agora distingue os dois, porque as ações são
  diferentes: ligar uma opção não é a mesma coisa que investigar assinatura e instalação.
- `Pedido::Procurar` e `Aviso::CandidatosEncontrados`, com o tipo `Candidato`. A interface não
  tinha como oferecer "parear" sem saber com quem. `Candidato` separa `rotulo` (para a tela) de
  `endereco` (para o serviço), porque o que identifica a máquina não é o que o usuário reconhece.

**Arquivos:** `crates/ir-ipc/` — `src/{lib,status,ui,agent,codec}.rs`,
`src/vocabulario/{mod,portador,capacidade,identidade}.rs`.

**Verificação.** `cargo test -p ir-ipc`: 32 testes passam. `cargo clippy` limpo,
`cargo xtask check` limpo.

**Decisões.** Os tipos deste crate são nomeados em português, e os de dentro (`ir-proto`,
`ir-session`) em inglês. A troca de idioma marca exatamente onde o produto termina e a
apresentação começa — não é estética, é um marcador de fronteira que aparece em toda linha de
código que a cruza.
