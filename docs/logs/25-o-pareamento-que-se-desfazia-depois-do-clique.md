# O pareamento que se desfazia depois do clique

**Data:** 2026-09-15

**Itens:** Etapa 4 — pareamento de ponta a ponta. Dois defeitos relatados no teste físico.

## O que quem usa viu

1. **No Windows**, depois de "Esquecer este computador", a janela continuou em "Conectando…", com o
   botão "Parear um computador" embaixo: nenhum par, e o serviço tentando conectar mesmo assim.
2. **No notebook**, na tela de pareamento, "São iguais" e "São diferentes" não faziam nada. Nenhuma
   reação, e o código continuava na tela.

## O que os registros mostraram

No Windows, depois de `par esquecido pela interface` às 19:54:39 UTC, um código de pareamento novo
atrás do outro, sem ninguém ter pedido:

| Hora (UTC) | Windows |
|---|---|
| 19:54:43 | código 334589 |
| 19:55:16 | "não" (os códigos das duas telas não batiam) |
| 19:55:18 | código 655688 |
| 19:57:18 | o código expirou; `conectando ao par`, e logo o código 127030 |

No notebook, **o clique chegava ao serviço**. Cada `confirmação recebida: sim` era seguida, de 1 a
3 s depois, por `enlace de rede caiu reason="handshake falhou"`:

| Confirmado (UTC) | Handshake desfeito |
|---|---|
| 19:54:09.3 | 19:54:10.4 |
| 19:54:47.3 | 19:54:50.7 |
| 19:55:23.1 | 19:55:26.7 |
| 19:58:29.4 | 19:58:32.7 |

No primeiro, o Windows chegou a registrar `par gravado` às 19:54:09.9, e o notebook não gravou nada:
um pareamento pela metade.

## As causas

Quatro, que se alimentavam:

1. **O serviço discava para parear sozinho.** `connect_if_possible` escolhia `ConnectMode::Pair`
   sempre que havia endereço e nenhuma chave gravada, e a reconexão o chamava a cada 3 s. Depois de
   esquecer, as duas máquinas tinham o endereço uma da outra na memória e nenhuma chave: cada uma
   passou a discar para parear a cada 3 s, e cada discagem punha um código novo na outra tela.
2. **O pareamento terminava, para o serviço, no clique.** `confirmar` apagava `pareamento_desde`
   antes de o outro lado responder. Com isso a proteção "no meio de um pareamento, não atrapalhar"
   se desligava, e a reconexão seguinte — em até 3 s — discava por cima do handshake que esperava a
   confirmação do outro computador. É o intervalo de 1 a 3 s da tabela.
3. **A janela nunca soube.** O aviso de pareamento sem sucesso só saía se ainda houvesse código
   esperando comparação, o que deixava de ser verdade depois do clique. E `ConfirmarPareamento`
   respondia `Feito` até para um código que já não valia. A tela ficava onde estava.
4. **Esquecer só apagava a chave.** A sessão e o enlace seguro ficavam de pé, e a reconexão
   continuava reiniciando a sessão sobre eles — a janela em "Conectando…".

## A prova, antes de corrigir

Em `crates/ir-daemon/src/actor/pareamento.rs`, com a bancada do serviço guardando agora o que ele
manda para a rede. Cinco testes falhavam no código de antes:

| Teste | Falhava com |
|---|---|
| `sem_par_gravado_o_servico_nao_disca_sozinho` | "parear só quando o usuário pede" |
| `depois_de_conferir_o_codigo_o_servico_nao_disca_por_cima` | `[ConfirmPairing(true), Connect { mode: Pair }]` — a discagem por cima, no mesmo passo |
| `se_o_enlace_cai_depois_de_conferir_a_janela_sai_da_espera` | nenhum `PareamentoConcluido { sucesso: false }` |
| `um_pareamento_conferido_que_nao_termina_vence_no_prazo_sem_acusar_codigos_diferentes` | `[ConfirmPairing(false)]` — recusaria dizendo "códigos diferentes" |
| `esquecer_o_par_encerra_a_conexao_e_nao_tenta_mais` | a sessão continuava fora de `Offline` |

Um protege o que funcionava e passava antes e depois: com par gravado, o serviço continua procurando
o par sozinho (`ConnectMode::Reconnect`).

## A correção

- **Parear só começa pela janela.** A reconexão automática só disca com par gravado, e sempre em
  `Reconnect`. `IniciarPareamento` é o único caminho para `ConnectMode::Pair`.
- **O pareamento vai do código na tela até o fim.** O serviço guarda um `Pareamento { desde,
  conferido }` no lugar do antigo `pareamento_desde`. `pareando()` cobre o pareamento inteiro, e
  `aguardando_confirmacao()` só a espera pelo usuário.
  Enquanto houver pareamento, a reconexão não disca. Termina com o par gravado, com o enlace caindo,
  com a recusa ou no prazo de 2 minutos.
- **No prazo, depois de conferido, o serviço desfaz o enlace** em vez de recusar: recusar mandaria
  "códigos diferentes", o sinal de alguém no meio, quando é só o outro lado que não respondeu.
- **A janela sempre fica sabendo.** O enlace que cai no meio do pareamento — conferido ou não — avisa
  `PareamentoConcluido { sucesso: false }`. Responder a um código que já não vale devolve a falha nova
  `Falha::PareamentoInterrompido` (índice 10). "São diferentes" devolve `Falha::CodigosDiferentes`,
  como o serviço simulado já fazia. Depois de "São iguais", a tela passa a "Aguardando o outro
  computador…", e uma falha sem motivo próprio aparece com o texto de `PareamentoInterrompido`.
- **Esquecer desliga.** Encerra o pareamento em curso, para a sessão pelo caminho que solta tudo e
  avisa o par, derruba o enlace seguro e avisa a janela. O endereço fica: é o candidato que "Procurar"
  oferece para parear de novo, e sem par gravado ninguém disca para ele.

## Arquivos

`crates/ir-daemon/src/actor/{mod,pareamento,pedidos,partes,bancada}.rs`, `crates/ir-ipc/src/falha.rs`,
`crates/ir-ui/src/janela.rs`.

## Verificação

- os cinco testes do defeito falhavam antes e passam depois; o de proteção passa nos dois momentos;
- responder a um código que já não vale, ou responder duas vezes, é recusado (teste novo, junto da
  correção);
- **490 testes** no workspace, nenhuma falha (eram 483); `cargo clippy --workspace --all-targets`,
  `cargo fmt --check` e `cargo xtask check` limpos (140 arquivos). O clippy recusou a primeira
  versão, com um quarto `bool` no ator: os dois campos do pareamento viraram um
  `Option<Pareamento>`.

## O que continua aberto

1. **Instalar nas duas máquinas** e refazer no hardware: esquecer no Windows e ver a janela parar;
   parear de novo e confirmar nas duas telas.
2. O computador que **não** esqueceu continua com a chave do outro e segue tentando reconectar a cada
   3 s. O que esqueceu registra esses quadros como `datagrama malformado`. Funciona, mas enche o diário.
3. Um código que vence **antes** do clique ainda é recusado com `ConfirmPairing(false)`, que a rede
   registra como `códigos diferentes` ([log 21](21-a-janela-que-travava-no-windows.md)).
