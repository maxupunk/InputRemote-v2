# A troca de chaves que travava a rede

**Data:** 2026-11-24

**Itens:** robustez do enlace de rede (queda sem volta depois de horas de uso)

## O sintoma

De tempos em tempos a conexão caía e às vezes não voltava mais. Na bancada (Windows
`SAMSUNG-MAXUEL` × Fedora `10.0.0.141`), depois de uma queda:

- o Windows dizia `fase: pronto`, `rota: udp`, "rede ouvida há 855 ms";
- o Linux abria a sessão (pela rede e pelo Bluetooth) a cada 3 s, e ela vencia em 1 s, sem fim;
- ligar o Bluetooth não mudava nada.

## O que os dados mostraram

**As quedas caíam na troca de chaves.** O Linux troca as chaves do enlace de rede a cada 10 min
(`…:32:32`). As quedas do dia foram em 12:40:32.8, 16:41:33.3 e 18:32:33.5 (UTC), cada uma cerca
de um segundo depois de uma troca prevista e que não aconteceu. Quando voltavam, a troca e a sessão
apareciam no mesmo milissegundo (12:41:31.904 e 12:41:31.904; 16:42:32.312 e 16:42:32.325).

**O Linux não mandava nada.** Num `tcpdump` de 15 s, o Linux recebeu cerca de 40 pacotes/s do
Windows (73/76 bytes: movimento de cursor, porque o Windows achava que estava controlando o Linux)
e **não enviou nenhum**. Ao mesmo tempo, o registro dizia `portador escolhido carrier=udp`.

## A causa

Três defeitos, um em cima do outro:

1. **O prazo do handshake se renovava** (`ir-net/src/handshake.rs`, `recv_from_peer`). Cada
   datagrama descartado no meio do handshake começava um `timeout(1,5 s)` novo. A troca de chaves
   rodava *dentro* do laço do endpoint e descartava os quadros de dados do par. Com o par mandando
   mais depressa que 1,5 s, a espera nunca vencia. **O endpoint ficava preso para sempre**: sem
   enviar, sem processar comando e sem avisar queda, então o serviço achava a rede de pé e nunca
   rediscava. As quedas de antes só voltavam porque o Windows acabava ficando 1,5 s em silêncio.
2. **Quem respondia trocava cedo demais.** O respondedor do `Noise_IK` mandava a resposta e passava
   a cifrar com as chaves novas na hora. Se a resposta se perdesse no Wi-Fi, cada lado ficava com
   um jogo de chaves, e nada abria do outro lado até a tentativa seguinte, um minuto depois.
3. **Quem pedia ficava surdo** por até 1,5 s, mais que o prazo de queda de 1 s. Bastava perder um datagrama da troca para a sessão cair.

## O que foi feito

- **Prazo absoluto** em `recv_from_peer`: um `timeout_at` para a chamada inteira.
- **Troca de chaves sem bloquear** (`endpoint/rechave.rs`): o pedido sai e o laço segue. A resposta
  é tratada como qualquer datagrama que chega, e sem ela a tentativa vence em 2 s sem custo nenhum.
  A espera entre tentativas caiu de 60 s para 10 s, já que falhar não custa mais nada.
- **Troca confirmada** (`Reserva`): quem pediu passa às chaves novas ao ler a resposta. Quem
  respondeu as guarda como *próximas* e continua cifrando com as atuais até abrir o primeiro quadro
  cifrado com as novas. As anteriores abrem o que chega por mais 30 s, então a troca não perde
  quadro e uma resposta perdida não desalinha os dois lados.
- **Rede de segurança no serviço** (`ir-transporte::Alcance::retomar`): se a sessão é refeita
  [`RETOMADAS_ANTES_DE_REDISCAR`] (5) rodadas seguidas sobre enlaces "de pé" sem subir, eles são
  derrubados e rediscados do zero, e o par troca o enlace dele pelo novo (`maybe_restart`). Não
  depende de saber *por que* o enlace mente.

O formato no fio não mudou. Contra um par ainda com o código antigo, uma resposta perdida desalinha
as chaves como antes, mas a próxima tentativa (10 s) ou a rede de segurança (≈15 s) refazem o
enlace. Antes, isso podia ficar travado para sempre.

**Arquivos:** `crates/ir-net/src/handshake.rs`, `crates/ir-net/src/endpoint.rs`,
`crates/ir-net/src/endpoint/rechave.rs`, `crates/ir-net/src/endpoint/rechave/testes.rs` (novo),
`crates/ir-net/src/endpoint/testes.rs`, `crates/ir-transporte/src/alcance.rs`,
`crates/ir-transporte/src/lib.rs`, `crates/ir-daemon/src/actor/alcance.rs`,
`crates/ir-daemon/src/actor/alcance/testes.rs`, `crates/ir-daemon/src/actor/enlace.rs`,
`crates/ir-daemon/src/commands.rs`.

## Verificação

- `cargo test -p ir-net -p ir-transporte -p ir-daemon`: tudo verde. Testes novos:
  `quadros_do_par_nao_renovam_o_prazo_do_handshake` (no código antigo **trava para sempre**, o que
  reproduz a bancada; agora vence dentro do prazo), `a_resposta_perdida_nao_desalinha_as_chaves`,
  `a_troca_se_confirma_com_o_primeiro_quadro_e_nada_se_perde`,
  `o_pedido_sem_resposta_vence_e_outro_sai_depois`, `so_rodadas_seguidas_sem_sessao_mandam_rediscar`
  e `o_enlace_de_pe_por_onde_a_sessao_nao_sobe_e_derrubado_e_rediscado`.
- `cargo clippy --workspace --all-targets` sem aviso; `cargo xtask check-limits` ok (o `ir-daemon`
  fica em 2499 de 2500 linhas de produção: `desconectar_todos` ficou mais curto para caber).
- Bancada: reiniciar só o serviço do Linux (ainda com o binário antigo) refez a sessão na hora,
  pela rede e pelo Bluetooth, o que confirma que o travado era o endpoint de rede do Linux.
  **Falta:** instalar a versão nova nos dois lados e deixar rodando por algumas horas, de
  preferência movendo o cursor entre as máquinas na virada dos 10 minutos.
