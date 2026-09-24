# A tela de bloqueio que recusava em silêncio, e agora é parede

**Data:** 2026-09-24

**Itens:** [ADR-0014](../adr/0014-controle-simetrico.md); [04, §6](../04-seguranca.md) — a política da
tela de bloqueio.

**O que foi relatado:** com o Fedora na tela de bloqueio, o mouse do Fedora ia para o Windows, mas o
do Windows não ia para o Fedora. Só funcionava depois de digitar a senha no Fedora.

## A causa

Não era injeção: era a política. A configuração do Fedora tinha `tela_de_bloqueio = false` — o
padrão, por segurança: o outro computador não digita na tela de bloqueio daqui sem a permissão. O
registro dizia *"digitação do par na tela de bloqueio recusada: a permissão está desligada"*.

A recusa estava certa; a experiência não:

1. **O Fedora só contava da recusa depois da primeira tecla barrada**, e não quando a tela bloqueava.
2. **A sessão do Windows não guardava a recusa.** O cursor atravessava a borda e ficava preso no
   Fedora, sem efeito nenhum, e a entrada do Windows suprimida.

## O que mudou

- **A recusa é anunciada assim que a tela protegida aparece** (`on_tela_protegida`), no Linux pelo
  `logind` e no Windows pelo desktop de entrada do agente — e deixa de valer na hora em que a
  permissão é ligada.
- **Enquanto o outro recusa, a borda que dá para ele é parede** (`Refusals` em
  `session/direction.rs`, parte de `may_cross`). O cursor fica deste lado.
- **Bloquear com o cursor do outro lá** devolve o controle a cada um: o lado que bloqueou retoma, e o
  outro, avisado, solta a supressão.
- **Uma sessão nova sabe**: a recusa é guardada na sessão e anunciada ao estabelecer; um
  `EnterScreen` que chegue mesmo assim é devolvido com `Reclaim`.
- **A tela diz o que fazer**: *"O outro computador está na tela de bloqueio, e não aceita o teclado e
  o mouse daqui ali: o cursor fica deste lado até ele ser desbloqueado. Para desbloqueá-lo daqui,
  ligue "Tela de bloqueio: Permitir" nas Preferências dele."*

Para caber no teto de tamanho do `ir-session`, `Route` foi para `ir-proto::route` — é um tipo de
valor sobre `Carrier`, e a sessão o reexporta com o mesmo nome.

## Como foi verificado

- `ir-session/tests/symmetric.rs`: bloqueado e sem permissão, a borda é parede, e desbloqueado volta
  a atravessar; bloquear sendo usado devolve o cursor; reconectar ainda bloqueado continua
  recusando.
- `actor/protegido.rs`: a recusa é anunciada na hora em que a tela bloqueia, e não com a permissão.
- 1 101 testes no Windows, 1 044 no container do Fedora; clippy e `xtask` limpos.

## Para digitar a senha do Fedora a partir do Windows

Ligue **Tela de bloqueio: Permitir** nas Preferências do **Fedora** (pede a senha de administrador).
É uma escolha de segurança por computador, e fica desligada até alguém decidir ligá-la.
