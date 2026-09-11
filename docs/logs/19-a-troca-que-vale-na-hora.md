# 19 — A troca de papel e de borda que vale na hora

**Data:** 2026-09-11

**Itens:** os dois defeitos que o [log 18](18-a-prova-no-notebook-e-a-troca-de-papel.md) registrou
fecharam; um terceiro, da mesma família, apareceu no desenho da correção e fechou junto.

## Os três defeitos

1. **A janela deixava escolher "servidor" num Linux.** O serviço aceitava, gravava, e no próximo
   reinício subia num papel sem captura, em que nada funciona.
2. **A troca de papel só valia quando o serviço reiniciava**, e ninguém ficava sabendo. No
   notebook, foram quase três horas entre o clique e o efeito.
3. **A borda tinha o mesmo defeito, pior.** `DefinirBorda` atualizava a borda que a janela mostra,
   mas a sessão guardava a antiga desde que nasceu. A tela dizia uma coisa e a travessia fazia
   outra — e nada no registro explicaria por que o ponteiro não atravessa onde a tela manda.

## O desenho

**Um mecanismo para os defeitos 2 e 3: recriar a sessão com o valor novo.** A sessão velha é
encerrada por `Session::stop`, que é o mesmo `tear_down` de toda queda: despede-se do par, emite o
`ReleaseAll` se o controle estava do outro lado e devolve a entrada local. Reaproveitar esse caminho,
em vez de escrever uma segunda soltura, é o que garante que as duas nunca divirjam — e é o ponto
mais sensível do produto, onde uma tecla presa deixa a máquina do outro inutilizável. A sessão nova
nasce com a mesma identidade (`LocalIdentity`, que o ator agora guarda) e o mesmo arranjo de telas
(guardado por `definir_telas`, que passou a ser a única porta de entrada das telas). Se havia enlace
seguro, o aperto de mão recomeça já com o papel novo.

A alternativa descartada foi um campo "pendente" no `Estado`. Acrescentar campo a uma estrutura
muda o layout no fio do `postcard` e exigiria versionar o contrato entre janela e serviço — muito
trabalho para anunciar um problema que dá para simplesmente não ter.

**Para o defeito 1, o serviço recusa o que a plataforma não sustenta.** Quem sabe se a plataforma
captura é o `ir-input`, que ganhou `capture_supported()`: o mesmo fato que `start_capture` só
descobre falhando, dito antes, sem instalar ganchos. Um teste garante que as duas respostas
concordam. A regra em si é uma função pura, `papel_sustentado(papel, captura)`. A recusa volta como
`Falha::PapelIndisponivel`, acrescentada no **fim** do contrato (índice 8), com o teste que trava os
índices atualizado.

**E na subida.** Um servidor gravado numa plataforma sem captura — o estado em que o notebook
ficou — sobe como cliente, registra em nível alto o que não aplicou e corrige o arquivo. Recusar-se
a subir seria pior: o `systemd` o reiniciaria em laço.

## Como foi verificado

- **Windows:** 455 testes; `clippy --all-targets`, `cargo xtask` e `fmt --check` limpos. Os testes
  de "vale na hora" rodam aqui, para papel e para borda — o da borda confere a sessão **em uso**,
  e não só o valor gravado. O teste de recusa retorna cedo: aqui o servidor é legítimo.
- **Linux, num Fedora 44:** 164 testes em `ir-input`, `ir-ipc`, `ir-session` e `ir-daemon`; clippy
  limpo. É onde a recusa existe de verdade, e rodaram:
  `servidor_sem_captura_e_recusado_sem_gravar_nada`,
  `na_subida_um_servidor_sem_captura_vira_cliente_e_o_arquivo_e_corrigido`,
  `unsupported_capture_is_declared_before_trying` e `o_indice_de_cada_falha_nao_muda`.

A imagem de compilação do Fedora tinha **sumido do Docker** e foi reconstruída antes da verificação;
sem ela, a metade da correção que só existe no Linux teria ficado sem prova.

**O que não foi verificado:** no notebook real, clicar "servidor" na janela e ver a recusa; trocar a
borda com o par conectado e ver a travessia acompanhar. Fica para a conferência com a pessoa.

## Decisões

- **Persistir antes de adotar.** A configuração nova é gravada e **só então** passa a valer; se a
  gravação falha, nem o arquivo nem a memória mudam. Antes a memória mudava primeiro, e uma
  gravação que falhasse deixava o serviço usando um valor que o próximo reinício perderia.
  `esquecer_par` passou a usar o mesmo caminho.
- **A sessão só é recriada se a gravação deu certo**, e só é encerrada se não estava desligada —
  encerrar uma sessão parada registraria uma "sessão encerrada" que não aconteceu.
- **O motivo da queda é `UserStopped`**: foi uma decisão de quem opera a máquina, e não uma falha.
- `Session::peer_edge()` entrou para os testes confirmarem a borda da sessão em uso, e levou o
  `session/mod.rs` a 403 linhas. Em vez de encurtar comentário para caber, as dez consultas só de
  leitura foram para `session/consultas.rs`; o arquivo principal ficou com o despacho de eventos.
- **O item do `PROGRESSO` mudou de texto.** Ele pedia que a janela avisasse da troca pendente; a
  correção escolhida eliminou a pendência, e o item passou a descrever isso.

**Arquivos:** `crates/ir-daemon/src/actor/{papel,mod,partes,pedidos,agente}.rs`,
`crates/ir-daemon/src/main.rs`, `crates/ir-input/src/lib.rs`, `crates/ir-ipc/src/falha.rs`,
`crates/ir-session/src/session/{mod,consultas}.rs`, `PROGRESSO.md`, `LOG.md`.
