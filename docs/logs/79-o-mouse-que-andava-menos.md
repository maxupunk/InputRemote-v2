# O mouse que andava menos, e o relógio que o Windows não dava

**Data:** 2026-11-25

**Itens:** desempenho do ponteiro (o cursor do outro computador "lento" de vez em quando)

## O sintoma

De vez em quando o cursor no outro computador ficava lento, com a latência medida entre 5 e 69 ms,
e voltava ao normal sozinho. Na hora da investigação a bancada estava sã: 1 000 pings do Windows ao
Fedora, um a cada 8 ms, com 1 perdido, mediana de 2 ms e pior caso de 15 ms. A rede tranquila não
explica o sintoma; o código explica três coisas que o agravam quando ela piora.

## O que se achou

1. **O filtro do ponteiro jogava fora o movimento que a rota dupla salvaria.** O canal 2 não tem
   confirmação, e o receptor aceitava só a amostra mais nova que a última aplicada. Na rota dupla, a
   cópia da rede se perde, a do Bluetooth chega 10 a 30 ms depois — e nesse meio a amostra seguinte
   já veio pela rede. A cópia que salvaria o movimento era descartada como velha. O movimento é
   relativo, então cada amostra perdida é um pedaço do caminho que o cursor não anda: com o Wi-Fi
   perdendo quadros, o cursor do outro lado andava menos do que a mão, que é o "lento".
2. **O intervalo de envio era o de um mouse comum.** Quem envia coalesce o movimento e manda no
   máximo uma amostra a cada 8 ms. Um mouse de 125 Hz manda a cada ~8 ms, com variação: o evento
   chegava um pouco antes do prazo, ficava para o seguinte, e o outro lado via 62 Hz com até 8 ms a
   mais de atraso.
3. **No Windows a batida de 5 ms saía a cada 13 ms.** O temporizador padrão do Windows anda em
   passos de 15,6 ms. Medido nesta máquina com o mesmo `tokio::time::interval` do ator: 152 batidas
   em 2 s (13,2 ms cada); com `timeBeginPeriod(1)`, 401 batidas (5,0 ms). Tudo o que a sessão faz
   por tempo ia duas a três vezes mais devagar do que o projetado: a confirmação pura de 20 ms, a
   retransmissão do piso, e o último pedaço de cada movimento, que só sai na batida seguinte.

## O que foi feito

- **Cada amostra vale uma vez, chegue quando chegar** (`ir-confiabilidade::pointer`). O filtro virou
  uma janela de repetição de 64 sequências, como a do transporte: a mais nova avança a janela, a
  atrasada que nunca foi vista também passa, e só a repetida — ou velha demais para saber — é
  descartada. Atrasada, só o movimento relativo vale; uma posição absoluta velha poria o ponteiro
  onde ele já não está. Mora no `ir-confiabilidade`, ao lado das outras peças de entrega: o
  `ir-session` estava no teto de linhas, e a peça só conhece `Sequence`.
- **O intervalo de envio caiu para 4 ms** (`Timings::pointer_interval`). Todo evento de um mouse de
  125 Hz sai na hora; um de 1 000 Hz é coalescido em 250 amostras por segundo, umas 15 KB/s, com
  folga até no Bluetooth.
- **O serviço pede o relógio de 1 ms no Windows** (`ir-servico::relogio`), pela vida do processo.
  Nos outros sistemas a guarda não faz nada.

O formato no fio não mudou, e a versão do protocolo continua a 11: o filtro é de quem recebe, e o
intervalo, de quem envia. Cada ponta melhora ao atualizar, mesmo com a outra ainda antiga.

**Arquivos:** `crates/ir-confiabilidade/src/pointer.rs` (novo), `crates/ir-confiabilidade/src/lib.rs`,
`crates/ir-session/src/session/{mod,frames,incarnation,link}.rs`, `crates/ir-session/src/config.rs`,
`crates/ir-servico/src/relogio.rs` (novo), `crates/ir-servico/src/lib.rs`,
`crates/ir-servico/Cargo.toml`, `crates/ir-daemon/src/main.rs`, `docs/03-protocolo.md` §4.2.

## Verificação

- `cargo test -p ir-session -p ir-confiabilidade -p ir-servico -p ir-daemon`: tudo verde. Testes
  novos do filtro: a cópia atrasada de uma amostra perdida ainda conta, a repetida não, a velha
  demais é descartada, o salto além da janela esquece as antigas, e a sequência dá a volta sem
  travar. Os da rota dupla (`a_pointer_sample_moves_the_cursor_once_on_a_dual_route` e o do
  datagrama duplicado) continuam provando que a cópia não move o cursor o dobro.
- `cargo clippy --workspace --all-targets` sem aviso, e `cargo clippy` dos crates mudados para
  `x86_64-unknown-linux-gnu` também; `cargo xtask check-limits` ok.
- O relógio: o programa de medição acima, nesta máquina.
- **Falta:** instalar nos dois lados e usar por um tempo, de preferência com o Wi-Fi sob carga
  (uma cópia grande passando), comparando o cursor com a rota dupla e só com a rede.
