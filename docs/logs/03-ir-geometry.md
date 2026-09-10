# `ir-geometry`: telas, bordas e mapeamento de coordenadas

**Data:** 2026-09-09

**Itens:** Etapa 2.2, inteira.

**O que foi feito.** Crate puro que responde às perguntas geométricas do produto: onde o
ponteiro está, se ele encostou na borda que dá para o par, e em que ponto da outra tela ele
deve aparecer.

| Módulo | Responsabilidade |
|---|---|
| `geom` | `Point` e `Rect`, com bordas inclusivas e aritmética saturada |
| `desktop` | o conjunto de monitores de uma ponta, e as consultas sobre ele |
| `crossing` | quando o controle passa, e por onde o ponteiro entra do outro lado |

### Decisões de projeto

**Nenhum ponto flutuante, em lugar nenhum.** `f32` não tem ordenação total, arredonda
diferente entre plataformas e tornaria um teste de mapeamento frágil. Toda a aritmética é
inteira, com `i64`/`u64` como intermediário onde o produto poderia estourar.

**A travessia viaja como fração, não como pixel.** A saída é medida em `0..=u16::MAX` ao
longo da borda. É o que faz o meio da borda de um monitor 4K virar o meio da borda de um
720p do outro lado. Em pixels, atravessar entre telas de tamanhos diferentes seria um salto.

**Só a borda do par atravessa.** As outras três prendem o ponteiro, como um monitor isolado
faria. Um KVM em que qualquer borda atravessa é um KVM que rouba o controle quando o usuário
mira num botão de canto.

**O monitor principal é campo, não busca.** `Desktop` guarda uma cópia do principal. O
invariante "sempre existe um monitor" deixou de ser comentário com `unreachable!` e passou a
ser estrutural — não há caminho de código que precise entrar em pânico nem devolver `Option`
para algo que sempre existe.

**Nenhuma função devolve coordenada inutilizável.** `nearest_valid` traz qualquer ponto para
a tela mais próxima: monitor desconectado com o ponteiro em cima, buraco de arranjo em L,
posição herdada de um arranjo antigo, lixo vindo do par. Há teste varrendo os quatro casos.

### Dois defeitos que os testes acharam

Vale registrar, porque nenhum dos dois teria aparecido em uso casual e os dois quebrariam o
produto em campo:

1. **`from_position` aplicava o recuo de borda.** `point_along` recua um pixel para dentro
   de propósito (impedir o ping-pong de travessia), e `from_position` a usava para converter
   posição do protocolo em ponto — deslocando o ponteiro um pixel a cada conversão. As duas
   responsabilidades foram separadas: `x_at`/`y_at` são exatos, `point_along` recua.

2. **A normalização truncava e comia o recuo.** Numa tela de 1920 px, um pixel vale 34
   unidades de fração; com truncamento nas duas conversões, o recuo de um pixel voltava a
   zero e o ponteiro reaparecia exatamente na borda — quicando de volta na amostra seguinte.
   Passou a arredondar para o mais próximo, com teste de regressão cobrindo de 800 a 7680 px.

O segundo é o tipo de defeito que se manifesta como "às vezes o ponteiro fica preso entre as
telas" e custa dias para reproduzir. Foi pego por um teste chamado
`entry_is_never_on_the_far_edge_so_it_cannot_bounce_back`, escrito antes de o defeito existir
— que é exatamente o argumento do núcleo sem E/S de [ADR-0004](../adr/0004-nucleo-sans-io.md).

**Arquivos:** `crates/ir-geometry/` (4 arquivos), `docs/02-arquitetura.md` (a seta
`ir-geometry ──► ir-proto` faltava no diagrama).

**Verificação.**

```text
cargo fmt --all -- --check                          OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test --workspace                              193 testes, 0 falhas
```

**Decisões.**
- Vários métodos de `Rect` deixaram de ser `const` porque precisam de `try_from` para
  converter dimensão sem cast que perca sinal. Const-ness não tinha uso real aqui; evitar o
  cast tem.
- `clippy::panic` foi liberado sob `cfg(test)` neste crate: teste que falha entra em pânico,
  e é assim que ele reporta.
