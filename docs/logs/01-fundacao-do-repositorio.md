# Fundação do repositório

**Data:** 2026-09-09

**Itens:** Etapa 1.1 — `git init` e arquivos de raiz; workspace Cargo; política de lints;
`rustfmt.toml` e `rust-toolchain.toml`; `PROGRESSO.md` e `LOG.md`.

**O que foi feito.** Repositório iniciado do zero, sem nenhum arquivo herdado do v1
(regra de [11 — Nada de legado](../11-nao-legado.md)). Workspace Cargo com
`resolver = "3"` e edição 2024, versão `0.1.0-dev` para deixar explícito que nada foi
lançado.

A política de lints foi transcrita de [09, §5](../09-padroes-de-codigo.md) para
`[workspace.lints]`, de forma que ela seja aplicada pelo compilador e não por combinação:

- `unsafe_code = "deny"` no workspace, com as exceções por crate previstas em
  [09, §4](../09-padroes-de-codigo.md) a serem declaradas quando os crates de plataforma
  existirem;
- `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `todo` e `unimplemented` em
  `deny` — a proibição de [09, §5](../09-padroes-de-codigo.md);
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
  ([00, §4](../00-licoes-do-v1.md)). O sufixo torna impossível confundir estado de
  desenvolvimento com versão entregue.
