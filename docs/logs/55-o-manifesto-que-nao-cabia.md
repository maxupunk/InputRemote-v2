# O manifesto que não cabia, e a cópia que recomeça sozinha

**Data:** 2026-10-02

**Itens:** [03, §6](../03-protocolo.md) — canal 5; [03, §8](../03-protocolo.md) — versionamento;
[01, §3.3](../01-visao-e-escopo.md) — a transferência não toca a entrada.

**O que foi pedido:** com o Windows de servidor e o Linux de cliente, copiar dados no Linux e levar
o mouse para o Windows antes de a cópia terminar mostrava "A cópia não atravessou · Jogo eletronica
2 · a conexão de arquivos caiu; teclado e mouse não foram afetados". Verificar, e deixar o fluxo de
copiar e colar o mais robusto possível.

## O que acontecia

No GNOME o sistema não avisa mudança de clipboard, então a cópia de arquivos **começa** quando o
controle sai do Linux ([ADR-0011](../adr/0011-clipboard-na-travessia.md)): levar o mouse ao Windows é
o gatilho, e não a causa.

A causa era o manifesto. Ele ia numa mensagem só, e um quadro TCP leva no máximo 65 519 bytes —
enquanto o protocolo aceita 10 000 itens com caminho de até 1 024 bytes. Uma pasta de jogo com uns
1 300 arquivos de nome comum já passa do quadro. O codec recusava o manifesto, e quem enviava tratava
**qualquer** erro de envio como queda do enlace: derrubava um enlace saudável, reconectava, e a tela
dizia que a conexão tinha caído. O teste novo mostra os dois lados: 3 000 arquivos não cabem num
quadro (`uma_pasta_de_jogo_nao_cabia_num_quadro_e_agora_vai_em_partes`), e com o fatiamento
desligado a travessia de verdade falha (`uma_pasta_com_milhares_de_arquivos_atravessa_pelo_canal_de_verdade`).

## O que mudou

- **O manifesto vai em partes** (protocolo versão 7). `ManifestPart` no fim do enum, as partes antes
  do `Manifest`, que fecha a lista e pede a resposta. Quem fatia é `ir_proto::message::manifest_messages`,
  pelo tamanho exato que o `postcard` dá a cada item; quem junta é `ir_files::PartesDoManifesto`, com
  teto contra um par que mande partes sem fim. Um manifesto de um quadro sai idêntico ao da versão 6,
  que continua aceita: os vetores dela seguem gravados, e os da 7 foram acrescentados.
- **"Não coube" deixou de ser "caiu".** `ir_transporte::FalhaDeEnvio` separa a mensagem que não vira
  quadro (`NaoCabe`, defeito daqui, enlace intacto) da falha do socket (`Enlace`). A decisão do que
  cada uma quer dizer para a cópia mora num lugar, `sessao::falhou_ao_enviar`.
- **Uma queda de verdade no meio da cópia não é mais o fim dela.** A cópia volta para a fila
  (`Fila::devolver`) e recomeça quando o enlace voltar; os dois lados mostram "Esperando a conexão
  voltar" (`Fase::AguardandoConexao`), que ainda pode ser cancelada. Dois limites, porque esperar para
  sempre também é falhar: a conexão tem um minuto para voltar (`PRAZO_PARA_VOLTAR`), e três quedas na
  mesma cópia a fazem desistir (`MAX_QUEDAS`). Só então aparece "a cópia não atravessou" — e o
  ajudante de clipboard, como antes, oferece a mesma cópia de novo na próxima travessia. As regras
  estão em `ir-transferencia/src/retomada.rs`, usadas por quem envia e por quem recebe.
- **Uma cópia pedida com o canal caído aparece na tela** esperando a conexão, com o mesmo prazo, em
  vez de esperar calada até ele voltar. Sem par pareado continua a recusa "não há par pareado", sem
  piscar a espera antes.

## Defeitos que apareceram no caminho

- **Um arquivo que deixava de ser lido no meio do envio** saía como "pronto": quem conduzia esperava
  trinta segundos pela conferência que o destino, cancelado, nunca mandaria, e então trocava o motivo
  de verdade na tela por "a conexão caiu". Agora é `Despejo::Falhou`, e o enlace segue servindo.
- **Trocar o destino no meio de uma cópia** (o par achado em outro endereço, ou outro par) largava
  quem a conduzia sem dizer nada: a cópia ficava marcada como em curso para sempre, e pedir a mesma
  cópia de novo era tomado por Ctrl+C repetido — nada saía. Agora a fila guarda a cópia em curso
  inteira e ela é recuperada (`Fila::abandonada`). A tarefa que lia o socket velho também seguia viva
  ao lado do enlace novo; agora é abortada ao sair de escopo (`AbortarAoSair`), e a recepção largada
  assim conta a espera pelo `Drop` (`EmCurso`), o mesmo caminho da queda.
- **A notificação do Linux segurava a mudança de fase pelo intervalo de dois segundos**: "esperando a
  conexão" podia não aparecer, e ficava na tela um andamento que já tinha parado. Mudança de fase
  conta na hora (`Notificador::mudou_de_fase`).
- **A barra de uma cópia que espera sem ter começado** aparecia cheia: total zero era lido como
  "pronto".

## Como foi verificado

- Windows: suíte inteira do workspace sem falhas, `clippy --workspace --all-targets` limpo e
  `cargo xtask check` dentro das regras.
- Contra o canal inteiro (TCP, Noise e codec, em `127.0.0.1`): a pasta de 3 000 arquivos chega
  completa; e uma cópia de 16 MB com o enlace derrubado no meio mostra a espera nos dois lados e chega
  inteira depois que ele volta (`tests/retomada.rs`).
- Os testes de integração do canal passaram a usar um módulo comum (`tests/comum`). O de Ctrl+C
  repetido copiava 24 MB e levava, já antes desta mudança, os mesmos doze segundos do prazo dele;
  passou a 8 MB, que ainda estão indo quando os pedidos repetidos chegam.

## O que ainda depende do hardware

Repetir o caso relatado na bancada — a pasta de jogo do Linux para o Windows, e o Wi-Fi desligado e
religado no meio de uma cópia grande — com o MSI e o RPM novos. Instalar o MSI pede administrador.

## O que fica para depois

- **Colar antes de a cópia chegar.** Hoje colar no outro computador antes do fim traz o que estava lá
  antes. O jeito certo é a promessa de conteúdo — o Windows entrega o clipboard só quando alguém cola
  (*delayed rendering*), e a colagem espera a cópia —, mas o Explorer e o GNOME têm prazos próprios
  para isso, e é decisão de produto à parte.
- **Retomar de onde parou**, em vez de recomeçar: pediria confiar numa montagem parcial entre dois
  enlaces, e a garantia do canal 5 é cópia certa.
