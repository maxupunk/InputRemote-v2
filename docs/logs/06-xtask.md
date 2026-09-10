# `xtask`: as regras de docs/09 viram executáveis

**Data:** 2026-09-09

**Itens:** Etapa 1.1 — `deny.toml` e CI. Etapa 1.2 — as três verificações.

**O que foi feito.** `cargo xtask check` roda três verificações e falha a build, não avisa:

| Verificação | O que garante |
|---|---|
| `check-limits` | linhas por arquivo, por função e por crate |
| `check-deps` | as setas de [02, §2](../02-arquitetura.md) e a pureza do núcleo |
| `check-logs` | nenhuma macro de log recebendo conteúdo digitado |

Mais `deny.toml` (licenças, avisos do RustSec, fontes) e o CI do GitHub, com clippy, testes e
documentação nos dois sistemas alvo, mais uma verificação cruzada para Linux.

### O que a implementação obrigou a decidir

**Os limites medem coisas diferentes, e isso agora está dito.** Ao rodar a verificação pela
primeira vez, `ir-proto` (4.872 linhas) e `ir-session` (3.846) estouraram o limite de 2.500 por
crate. Havia três saídas, e duas eram erradas:

- *dividir os crates* seria cargo cult: `ir-proto` é **uma** responsabilidade, e quebrá-lo em
  `ir-proto-types` e `ir-proto-codec` só espalharia a mesma coisa por dois lugares;
- *aumentar o número* seria desistir da regra na primeira vez que ela incomodou.

A saída certa foi perceber que o limite **media a coisa errada**. Ele existe para conter
acúmulo de responsabilidade — é ele que impediria o crate de 10.491 linhas do v1. Só que
documentação não acrescenta responsabilidade (reduz o custo de entender o que já está lá) e
teste não acrescenta responsabilidade (acrescenta confiança). Contar qualquer um dos dois cria
pressão para escrever menos deles, que é o oposto do que este projeto quer.

O limite por crate passou a contar **só linhas de código de produção**: sem testes, sem
comentários, sem linhas em branco. Com isso, os dois crates passam com folga — e a regra
continua pegando crescimento real. O limite por **arquivo** continua contando tudo, porque ali
o que se protege é a navegabilidade, e um arquivo longo é longo de rolar mesmo que a maior
parte seja documentação. [09, §1](../09-padroes-de-codigo.md) foi reescrito para dizer qual
limite mede o quê, e por quê.

**Um defeito no próprio verificador.** O detector de funções longas usava
`opens.saturating_sub(closes)` para acompanhar profundidade — que satura em zero e portanto
**nunca decrementa**. Nenhuma função era detectada. Foi pego pelo teste
`every_flavour_of_signature_is_recognised`, que exercita `fn`, `pub fn`, `pub(crate) fn`,
`const fn` e `async fn`. Passou a usar delta com sinal.

Vale registrar porque é o argumento a favor de testar ferramenta de verificação: uma que não
acusa nada é indistinguível de uma que funciona, e a diferença só aparece quando já é tarde.

**As heurísticas são declaradas como heurísticas.** A contagem de funções e a exclusão de
módulos de teste contam chaves, não constroem árvore sintática. Erram em macro que abre chave
sem fechar na mesma linha, e acertam no resto — e erram **para mais**, o que faz uma função ou
um crate parecerem maiores. É o lado seguro de errar, e está escrito no código.

**Arquivos:** `xtask/` (5), `deny.toml`, `.cargo/config.toml` (o alias `cargo xtask`),
`.github/workflows/ci.yml`, `docs/09-padroes-de-codigo.md`.

**Verificação.**

```text
cargo fmt --all -- --check                              OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test --workspace                                  328 testes, 0 falhas
cargo xtask check                                       65 arquivos, nenhuma violação
```

**Decisões.**
- `spikes/` fica fora das verificações: é código descartável de prova de conceito
  ([08, §2](../08-plano-de-implementacao.md)), e aplicar limites a ele atrasaria a resposta a
  perguntas sem melhorar nada.
- `xtask/src/logs.rs` está isento da própria verificação de log, porque precisa citar os nomes
  proibidos para poder procurá-los.
- A lista de nomes proibidos em log é deliberadamente ampla. Um falso positivo custa uma
  renomeação; um falso negativo custa a senha do usuário num arquivo de log.
