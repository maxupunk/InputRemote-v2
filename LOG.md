# Log de implementação

Registro append-only. Cada entrada corresponde a um ou mais itens marcados em
[PROGRESSO.md](PROGRESSO.md). Entrada nova vai no fim do arquivo, nunca no meio.

Formato de cada entrada:

```
## AAAA-MM-DD — <título curto>
**Itens:** <quais itens do PROGRESSO fecharam>
**O que foi feito:** <descrição>
**Arquivos:** <lista>
**Verificação:** <o comando que rodou e o resultado, ou a inspeção feita>
**Decisões:** <o que foi decidido no caminho, se houver>
```

---

## 2026-09-09 — Fundação do repositório

**Itens:** Etapa 1.1 — `git init` e arquivos de raiz; workspace Cargo; política de lints;
`rustfmt.toml` e `rust-toolchain.toml`; `PROGRESSO.md` e `LOG.md`.

**O que foi feito.** Repositório iniciado do zero, sem nenhum arquivo herdado do v1
(regra de [11 — Nada de legado](docs/11-nao-legado.md)). Workspace Cargo com
`resolver = "3"` e edição 2024, versão `0.1.0-dev` para deixar explícito que nada foi
lançado.

A política de lints foi transcrita de [09, §5](docs/09-padroes-de-codigo.md) para
`[workspace.lints]`, de forma que ela seja aplicada pelo compilador e não por combinação:

- `unsafe_code = "deny"` no workspace, com as exceções por crate previstas em
  [09, §4](docs/09-padroes-de-codigo.md) a serem declaradas quando os crates de plataforma
  existirem;
- `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `todo` e `unimplemented` em
  `deny` — a proibição de [09, §5](docs/09-padroes-de-codigo.md);
- `clippy::all` e `clippy::pedantic` em `warn`, com `module_name_repetitions` e
  `must_use_candidate` liberados por serem ruído neste projeto.

`rust-toolchain.toml` fixa 1.96.0 e os dois alvos, para que a build não dependa do que
está instalado na máquina de quem compila.

**Arquivos:** `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`,
`.gitattributes`, `LICENSE`, `PROGRESSO.md`, `LOG.md`.

**Verificação.** `rustc 1.96.0`, `cargo 1.96.0`, `git 2.55.0` confirmados na máquina.
`git init` executado. Os membros do workspace ainda não existem — a build só é exercida na
entrada seguinte, junto com o primeiro crate.

**Decisões.**
- `panic = "abort"` no perfil de release. Um serviço `SYSTEM` que entra em pânico deve
  morrer e ser reiniciado pelo gerenciador de serviços, não desenrolar a pilha num estado
  parcial com teclas possivelmente pressionadas. O `ReleaseAll` de segurança é
  responsabilidade do serviço ao reiniciar, e do agente ao detectar a queda do pipe.
- `strip = "symbols"` e `lto = "thin"` para manter o binário pequeno, já que ele é carregado
  cedo no boot.
- Versão `0.1.0-dev` em vez de `0.1.0`: o v1 lançou `0.1.0` com itens em aberto no roadmap
  ([00, §4](docs/00-licoes-do-v1.md)). O sufixo torna impossível confundir estado de
  desenvolvimento com versão entregue.

---

## 2026-09-09 — `ir-proto`: o núcleo do protocolo, puro e testado

**Itens:** Etapa 1.1 — `clippy.toml`. Etapa 2.1 — quase toda; o que ficou aberto e por quê
está em [PROGRESSO.md](PROGRESSO.md).

**O que foi feito.** O crate `ir-proto` completo: 19 arquivos em `src/`, nenhum acima de 336
linhas. Nenhuma dependência de E/S, de relógio ou de sistema operacional — `postcard`,
`serde` e `thiserror`, e nada mais.

Organização por responsabilidade única, uma ideia por arquivo:

| Módulo | Responsabilidade |
|---|---|
| `limits` | todo tamanho máximo do protocolo, com a origem de cada número |
| `error` | o único tipo de erro, sem variante que carregue conteúdo do fio |
| `version` | versão e negociação |
| `ids` | identificadores opacos e distintos entre si |
| `carrier` | RFCOMM, UDP, TCP e o que cada um garante |
| `channel` | os seis canais e a política de o que viaja por onde |
| `input/` | tecla, modificador, botão, ponteiro, roda, estado |
| `screens` | arranjo de telas e bordas |
| `peer/` | nome, capacidades e os níveis N0–N3 |
| `message/` | o catálogo, um enum por canal |
| `frame` | envelope de canal, sequência e confirmação |
| `codec` | codificação e decodificação |

### Decisões de projeto tomadas durante a implementação

**O canal é a variante externa de `Message`.** A tabela de "que mensagem pode ir em que
canal" de [03, §4](docs/03-protocolo.md) deixou de ser verificação em tempo de execução e
passou a ser garantia do compilador: não há como construir um bloco de arquivo no canal do
ponteiro. E como `postcard` codifica a posição da variante, o primeiro byte do fio passou a
ser o número do canal de graça — com um teste guardando a correspondência, porque reordenar
as variantes quebraria isso em silêncio.

**Aritmética de sequência da RFC 1982.** `Sequence::is_newer_than` não compara inteiros.
Depois de 2³² mensagens o contador dá a volta, e comparação ingênua faria o canal do ponteiro
travar para sempre naquele instante — bug que só aparece depois de horas de uso e por isso
mesmo é caríssimo de achar em produção. Há teste atravessando a fronteira da volta.

**Modificadores em dois lugares, de propósito.** `InputState::apply_key` registra um
modificador no bitmap que viaja em toda mensagem **e** no conjunto de teclas que o injetor
solta. Sincronizar os dois numa função só é o que impede o estado inconsistente que produz
modificador preso.

**Reconciliação idempotente testada como propriedade.** `PressedKeys` e `Modifiers` têm teste
que reconcilia dois estados, confere o resultado e reconcilia de novo esperando nenhuma ação.
É a base da meta de zero teclas presas.

**Limites conferidos antes de alocar.** `ScreenLayout::new`, `PressedKeys::from_vec` e
`validate_manifest` verificam a contagem anunciada **antes** de percorrer ou alocar. Há teste
que passa uma lista simultaneamente inválida e acima do limite, e exige que o erro seja o de
contagem — provando que a verificação barata vem primeiro. Isto importa porque a contagem vem
de um par remoto e o decodificador roda como `SYSTEM`.

**Travessia de diretório barrada no crate puro.** `ManifestItem::is_safe_path` recusa caminho
absoluto, `..`, `.`, componente vazio, barra invertida, raiz de unidade do Windows e byte
nulo. Ficar no crate puro permite testá-la exaustivamente sem tocar em disco.

**Erros que não são oráculo.** Nenhuma variante de `ProtoError` carrega bytes ou texto vindos
do fio, e `ErrorCode` — o que vai para o par — é enum fechado. O detalhe fica no log local.
Um decodificador que descreve em detalhe a entrada inválida ajuda quem está sondando.

**Vetores gravados.** `tests/vectors.rs` fixa os bytes exatos de 16 quadros da versão 1. É a
única proteção contra a falha mais perigosa deste protocolo: `postcard` é posicional, então
reordenar campos produz bytes que o outro lado decodifica **com sucesso** e interpreta
errado. Teste de ida e volta não pega, porque as duas pontas do teste mudam juntas. O
cabeçalho do arquivo explica o que fazer quando ele falhar, para que ninguém "conserte" o
teste apagando o vetor.

**Limites de tamanho como asserção de compilação.** As relações entre as constantes de
`limits` são `const _: () = assert!(...)`, não teste. Um limite incoerente deixa de compilar,
o que é melhor que falhar num teste que alguém pode marcar como ignorado.

**Arquivos:** `crates/ir-proto/` (19 em `src/`, 1 em `tests/`), `clippy.toml`.

**Verificação.**

```text
cargo fmt --all -- --check                             OK
cargo clippy -p ir-proto --all-targets -- -D warnings  OK
cargo test -p ir-proto                                 147 testes, 0 falhas
```

Maior arquivo: 336 linhas, contra o limite de 400 de
[09, §1](docs/09-padroes-de-codigo.md). Nenhuma função acima de 60 linhas.

Três coisas foram corrigidas pelos próprios limites do projeto, e vale registrar porque é a
regra funcionando: `peer.rs` chegou a 397 de 400 linhas e foi dividido em
`peer/{name,level,capabilities}.rs`; a tabela de vetores passou de 60 linhas e virou um grupo
por canal; e um teste meu estava errado — afirmava que a saída de `HidUsage::Display` não
contém a letra `a`, sem notar que a palavra `usage` contém.

**Decisões.**
- `ir-geometry` e `xtask` foram **temporariamente retirados** dos membros do workspace para
  não bloquear a build enquanto não existem. Precisam voltar quando forem criados; anotado em
  PROGRESSO 1.2 e 2.2.
- A seção de documentação de erro chama-se `# Errors`, em inglês, mesmo com o corpo em
  português: é convenção do rustdoc, como `# Panics` e `# Safety`, e o clippy a exige.
- `clippy.toml` traz `doc-valid-idents` com os nomes próprios do projeto, para que o lint de
  documentação não peça crase em nome próprio, e os limites de
  [09, §1](docs/09-padroes-de-codigo.md) que o clippy já sabe verificar
  (argumentos, linhas por função, complexidade, aninhamento). Os demais ficam para o
  `xtask`.
