# 09 — Padrões de código

Estas regras não são estilo. Cada uma corresponde a uma falha medida no v1
([00](00-licoes-do-v1.md)), e o CI as faz cumprir. Uma regra que só existe no documento
é uma regra que já foi quebrada.

## 1. Limites de tamanho — verificados pelo CI

| Limite | Valor | Ação ao estourar |
|---|---:|---|
| Linhas por arquivo `.rs` | 400 | falha de build |
| Linhas por função | 60 | falha de build |
| Linhas por crate (`src/`) | 2 500 | falha de build |
| Parâmetros por função | 5 | falha de build |
| Complexidade ciclomática por função | 15 | falha de build |
| Profundidade de aninhamento | 4 | falha de build |

Referência do v1: `controller.rs` tinha 4.545 linhas — onze vezes o limite. Nenhum
commit isolado criou aquele arquivo; ele cresceu porque nada o impedia. A verificação
roda em `cargo xtask check-limits`.

Estourar um limite é sinal de que falta uma abstração, não de que o limite está errado.
Aumentar um limite exige ADR.

## 2. Regra das setas de dependência

As setas de [02, §2](02-arquitetura.md) são verificadas por `cargo xtask check-deps`,
que lê os `Cargo.toml` e falha se:

- `ir-proto`, `ir-session` ou `ir-geometry` dependerem de `tokio`, de qualquer crate de
  E/S, de qualquer API de sistema operacional ou de relógio de parede;
- `ir-ui` depender de qualquer coisa além de `ir-ipc` e da interface;
- um crate de plataforma depender de outro crate de plataforma;
- surgir um ciclo.

## 3. Pureza do núcleo

Em `ir-proto`, `ir-session` e `ir-geometry`:

- `#![forbid(unsafe_code)]`;
- nenhuma leitura de tempo: `Instant` entra por parâmetro, sempre;
- nenhuma leitura de ambiente, arquivo, rede ou variável global;
- nenhuma aleatoriedade não injetada;
- nenhum `async`;
- toda função pública com teste.

O critério prático: `cargo test -p ir-session` roda em menos de dois segundos, em
qualquer máquina, sem nenhum periférico. Se deixar de rodar, alguma coisa vazou para
dentro do núcleo.

## 4. `unsafe`

`unsafe_code = "deny"` no workspace. As exceções são declaradas por crate e por módulo:

| Crate | Módulos que podem usar `unsafe` |
|---|---|
| `ir-input` | `windows::sendinput`, `windows::hooks`, `windows::rawinput`, `linux::uinput`, `linux::evdev` |
| `ir-bt` | `windows::winsock` |
| `ir-daemon` | `windows::service`, `windows::spawn_agent` |

Dentro deles:

- todo bloco `unsafe` tem um comentário `// SAFETY:` dizendo qual invariante o torna
  válido — não o que o código faz, e sim por que é seguro;
- cada bloco cobre a menor região possível;
- toda chamada de FFI é embrulhada numa função segura no mesmo módulo, e o resto do crate
  usa só o embrulho;
- `unsafe` fora dessa lista é falha de build.

## 5. Erros

- bibliotecas usam `thiserror` com enums específicos; binários usam `anyhow`;
- `unwrap()`, `expect()` e `panic!()` são proibidos fora de testes, por lint `deny`;
- indexação que possa entrar em pânico (`v[i]`) é proibida em `ir-proto` — use `get`;
- todo erro que chega ao usuário tem: o que aconteceu, em qual componente, e o que fazer
  agora. Mensagem sem a terceira parte não passa na revisão;
- erro no caminho do decodificador nunca vaza detalhe interno para o par remoto: quem
  recebe vê um código, e o detalhe fica no log local.

## 6. Concorrência

- um dono por estado, comunicação por canal ([02, §4](02-arquitetura.md));
- `Arc<Mutex<...>>` exige justificativa na descrição do PR; `Arc<Mutex<Estado>>` de
  estado de produto é proibido;
- toda fila é limitada, e a política de saturação é escolhida explicitamente na criação:
  `DropOldest` para ponteiro, `FailLink` para tudo que é confiável;
- nada de `std::thread::sleep` em código assíncrono;
- nada de bloqueio dentro de `tokio` sem `spawn_blocking`.

## 7. Caminho de latência

Funções no caminho do evento de entrada são marcadas `#[doc = "hot path"]` e obedecem a
[02, §6](02-arquitetura.md): sem alocação, sem log síncrono, sem lock disputado, sem E/S.
O CI roda um *benchmark* de regressão que falha se a mediana piorar mais de 15% em
relação à referência gravada.

## 8. Logs

- `tracing`, com escritor sem bloqueio;
- **nunca** conteúdo de tecla, caractere, HID Usage, coordenada ou clipboard acima de
  `trace` — ver [04, §7](04-seguranca.md);
- um lint próprio (`cargo xtask check-logs`) procura por macros de log que recebam campos
  de tipos de entrada e falha a build;
- cada log traz o componente e o identificador da sessão;
- mensagem de log é frase, não sigla.

## 9. Testes

- toda função pública dos crates puros tem teste;
- todo bug corrigido entra com um teste que falha antes da correção — sem exceção;
- todo tipo do protocolo tem teste de ida e volta, de tamanho máximo e **vetores gravados**
  — bytes de referência por versão, que falham se a codificação mudar sem incremento de
  `protocol_version`. Sem eles, `postcard` quebra compatibilidade em silêncio
  ([03, §9](03-protocolo.md));
- backends de plataforma têm testes atrás de `#[cfg]`, e o CI roda os dois sistemas;
- detalhes em [10](10-testes-e-validacao.md).

## 10. Comentários e documentação

- `missing_docs = "warn"` no workspace; itens públicos dos crates puros exigem documento;
- comentário explica **por quê**, nunca **o quê** — o código já diz o quê;
- toda constante mágica tem comentário com a origem do número (documentação, medição ou
  decisão) e a data;
- todo contorno de comportamento de sistema operacional cita a armadilha correspondente
  em [05](05-windows.md) ou [06](06-linux.md). Sem essa referência, o próximo a ler vai
  "limpar" o contorno e reintroduzir o defeito.

## 11. Commits e revisão

- um commit resolve uma coisa e passa no CI sozinho;
- mensagem no imperativo, dizendo o efeito, não o arquivo mexido;
- PR que aumenta um limite da §1, adiciona dependência ou muda `unsafe` precisa de ADR.

Lista de verificação da revisão:

1. isto poderia estar num crate puro em vez de num crate de E/S?
2. o estado ficou com um dono só?
3. o caminho de erro libera as teclas?
4. o que acontece se o par sumir exatamente aqui?
5. isto registra em log alguma coisa que o usuário digitou?
6. um teste falharia se este código fosse revertido?

A pergunta 3 é a que mais pega defeito real nesta classe de produto.
