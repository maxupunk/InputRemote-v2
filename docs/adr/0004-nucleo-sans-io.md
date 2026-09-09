# ADR-0004 — Núcleo de sessão sem E/S

**Status:** aceito · **Data:** 2026-09-09 · **Substitui:** nada

## Contexto

No v1, a lógica de produto vivia em `controller.rs` (4.545 linhas) e `application.rs`
(3.759 linhas), dentro do crate da interface, misturada com `async`, sockets, chamadas de
sistema operacional e o ciclo de repintura da janela.

O efeito não foi só estético. Um bug de "tecla presa depois de reconectar" só podia ser
reproduzido com dois computadores, um rádio e a sequência exata de eventos. Escrever um
teste para ele era, na prática, impossível — e um bug que não vira teste volta.

## Decisão

`ir-session`, `ir-proto` e `ir-geometry` são crates **sem E/S**. A sessão é uma função
sobre estado:

```rust
impl Session {
    pub fn step(&mut self, input: Input) -> CommandBatch;
}
```

Entra um evento, sai uma lista de comandos. Nesses crates é proibido: ler o relógio,
abrir socket, tocar em arquivo, chamar API de sistema, usar `async`, usar aleatoriedade
não injetada. `Instant` entra por parâmetro, sempre.

Toda a E/S vive na periferia e apenas converte: bytes → `Input`, `Command` → bytes.

## Alternativas descartadas

**Núcleo assíncrono com `tokio` dentro.** É o caminho natural em Rust e é o que o v1 fez.
Um teste passa a exigir um runtime, temporizadores reais e um transporte falso; cenários
de *timeout* passam a levar segundos de relógio real; a ordem de execução deixa de ser
determinística. O custo aparece devagar e não tem volta.

**Ator com canais, mas com E/S dentro.** Melhora o isolamento de estado sem resolver a
testabilidade — que é o problema real.

**Manter a lógica na interface, só que organizada.** Foi o que o v1 tentou. Sem uma
fronteira que o compilador imponha, a organização se desfaz commit a commit.

## Consequências

**Boas.**
- `cargo test -p ir-session` roda em menos de dois segundos, em qualquer máquina, sem
  periférico nenhum.
- Tempo é simulado: um cenário de reconexão de 30 segundos roda instantaneamente.
- Um bug relatado vira teste em minutos, porque o estado é reproduzível e serializável.
- Fuzzing da máquina de estados fica trivial: é só uma sequência de `Input`.
- Não existe `Arc<Mutex<Estado>>`, logo não existe corrida sobre o estado de produto.
- Trocar de portador, de runtime ou de sistema operacional não toca no núcleo.

**Ruins, e aceitas.**
- Escrever o núcleo custa mais no começo: eventos e comandos precisam ser tipos
  explícitos, não chamadas diretas.
- A camada de tradução entre `Command` e E/S é código adicional que não existiria.
- É mais fácil de errar na primeira semana e muito mais barato a partir do primeiro mês —
  a aposta é explícita.
- Exige disciplina: a primeira vez que alguém precisar do relógio dentro do núcleo, a
  tentação será ler o relógio. O CI recusa ([09, §2](../09-padroes-de-codigo.md)).
