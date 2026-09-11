# 18 — A prova no notebook, e a troca de papel que ninguém viu

**Data:** 2026-09-11

**Itens:** a conferência visual que o [log 17](17-a-janela-que-volta-e-o-grupo-que-vale-na-hora.md)
deixou pendente foi feita; dois defeitos novos entraram no `PROGRESSO`.

## A prova, na máquina real

Notebook Fedora 44 com GNOME, pacote `...20260911173654` (commit `63bea58`), com a pessoa olhando a
tela e confirmando o que via.

| Passo | Banco de usuários | Grupos do processo da janela | O que o serviço fez | Na tela |
|---|---|---|---|---|
| tira do grupo, reinicia o serviço | sem o grupo | **com** o grupo, desde o login | `interface recusada` a cada tentativa | faixa *"Este usuário não tem permissão…"* — confirmada pela pessoa |
| devolve o grupo, nada reiniciado | com o grupo | com o grupo | `interface conectada` ~9 s depois; nenhuma partida do serviço; mesma janela | a faixa some |
| volta o papel para cliente, reinicia | — | — | `papel cliente`, dois dispositivos `uinput`, mesma janela reconectada | a faixa de serviço parado aparece e some |

A primeira linha é a pegadinha do GNOME **ao contrário**: o processo da janela carrega o grupo, e
mesmo assim é recusado, porque o banco de usuários diz que não. É a prova mais direta de que quem
decide é a consulta na hora, e não o que o processo herdou. Os ~9 s da segunda linha batem com o
desenho: recusada por permissão, a janela espera dez vezes o intervalo normal antes de tentar de
novo.

Entre a recusa e a devolução, a janela foi fechada e reaberta pela pessoa (o pid mudou). A
devolução foi medida nessa janela reaberta, que já estava aberta antes do `usermod` e continuou a
mesma até entrar — então a prova de "sem sair da sessão, sem reiniciar, sem reabrir" vale.

## A troca de papel que ninguém viu

Ao conferir a máquina antes da prova, a configuração do Linux estava com `role = "server"`. Ele era
cliente quando o trabalho parou, e um Linux ainda não tem captura para ser servidor. O registro do
serviço contou a história:

| Hora | O que aconteceu |
|---|---|
| 12:01:25 | o notebook liga e o serviço sobe como **cliente** |
| 12:02:08 | a janela conecta |
| **12:03:19** | a configuração é gravada — **sem nenhuma linha no registro** |
| 14:47:14 | o pacote novo reinicia o serviço, que sobe como **servidor** |

A borda continuou a mesma, então o que se gravou às 12:03 foi o papel, por um clique na janela. Não
houve registro porque essa versão do serviço não registrava a troca — o buraco que o commit
`370325f` já tinha fechado, mas que ainda não estava no pacote instalado. A pessoa pediu, e o Linux
voltou a ser cliente.

O episódio expôs dois defeitos de produto, e não de uso:

1. **A janela deixa escolher "servidor" num Linux.** O serviço aceita, grava, e no próximo
   reinício sobe num papel em que nada funciona — sem avisar ninguém.
2. **A troca de papel só vale quando o serviço reinicia, e a janela não diz isso.** A mudança fica
   pendente e invisível, e aparece horas depois como um problema sem causa aparente. Aqui, foram
   quase três horas entre o clique e o efeito.

Nenhum dos dois foi corrigido junto, por decisão. O primeiro exige escolher que falha e que
instrução o contrato devolve. O segundo toca o `Estado` publicado, e acrescentar campo a uma
estrutura muda o layout no fio do `postcard` — o mesmo cuidado que travou o índice das falhas.
Merecem desenho próprio, não um remendo no meio de um teste físico.

**Arquivos:** `LOG.md`, `PROGRESSO.md`, este arquivo. Nenhum código.

**Verificação:** a tabela da prova acima, lida do registro do serviço e confirmada na tela pela
pessoa; a linha do tempo da troca de papel, lida do registro do `systemd` e da data de gravação de
`/var/lib/inputremote/config.toml`.
