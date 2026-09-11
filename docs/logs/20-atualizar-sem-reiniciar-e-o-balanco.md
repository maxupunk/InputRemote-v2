# Atualizar sem reiniciar, e o balanço do que falta

**Data:** 2026-09-11

**Itens:** atualizar por cima sem reiniciar o Windows (`[~]`, falta a instalação de verdade);
`tracing` com escritor sem bloqueio; configuração com escrita atômica; relatório de diagnóstico por
campos permitidos; caminhos de sistema (`[~]`); os três itens da PoC-5 que a Etapa 3 já cumpria; o
"um comando gera tudo" da PoC-6 (`[~]`).

## O que aconteceu

Instalar o MSI novo por cima do antigo parou num aviso do Windows Installer: *"The setup must update
files or services that cannot be updated while the system is running... a reboot will be
required"*. A máquina não podia ser reiniciada.

O estado no momento do aviso:

- o serviço `InputRemote` rodando na sessão 0;
- o agente `inputremote-agent` rodando como SYSTEM na sessão 1, sem janela;
- a janela da interface **fechada**;
- nenhuma troca de arquivo do InputRemote pendente em `PendingFileRenameOperations` — o aviso não
  vinha de uma instalação anterior mal terminada.

**A causa.** O Windows Installer consulta o *Restart Manager* no começo, em `InstallValidate`,
**antes** de parar qualquer serviço. O Restart Manager encontra o agente com os arquivos abertos, vê
que é um processo sem janela e de outra conta — que nenhum "fechar programa" alcança — e conclui que
só reiniciando. Mas, na ordem real da instalação, o `ServiceControl Stop` do serviço vem antes da
cópia dos arquivos, e o agente sai sozinho quando o canal dele fecha. O aviso era um falso positivo
para este produto.

## O que foi feito

**1. O instalador não consulta mais o Restart Manager** (`MSIRESTARTMANAGERCONTROL = Disable`).
Sem ele, o Windows Installer volta a conferir só programas com janela: a interface aberta ainda
aparece na lista para a pessoa fechar, que é o comportamento certo.

**2. O serviço para limpo, e só se declara parado depois.** Antes, ao receber "parar", o serviço
reportava `Stopped` e o processo acabava. O par só percebia pelo próprio prazo de queda, e o agente
só saía quando o sistema fechava o canal — **depois** de o serviço constar como parado, que é
justamente o instante em que um instalador vai trocar o arquivo do agente. Agora:

- o serviço reporta `StopPending` com prazo de 5 s;
- um canal de parada chega ao laço do ator, que encerra a sessão pelo caminho de toda queda
  (`Session::stop`: soltar tudo, avisar o par), manda `Encerrar` ao agente e dá 300 ms para isso
  sair pelos canais;
- só então o serviço reporta `Stopped`. Se a parada limpa travar, o prazo vence e ele se declara
  parado mesmo assim, para o SCM não achar que o serviço travou.

A parada foi para um módulo próprio (`actor/parada.rs`), com dois testes: parar com a sessão de pé
a derruba e dispensa o agente; parar sem sessão ainda dispensa o agente. A montagem do serviço para
teste virou uma bancada compartilhada (`actor/bancada.rs`), que os testes de papel também usam.

Para **esta** atualização em particular, quem para é o serviço antigo, que não tem a parada limpa.
A correção 1 basta: o serviço antigo para, o agente antigo sai quando o canal fecha, e os arquivos
estão livres na hora da cópia. A correção 2 vale para as próximas.

**3. O registro não bloqueia mais.** O `tracing` passou a escrever por fila (`tracing-appender`):
uma linha nunca espera disco ou console, e quem registra pode ser o laço da sessão, que bate a cada
5 ms. Como serviço do Windows, o registro vai para `%ProgramData%\InputRemote\logs`, um arquivo por
dia, sete guardados — ninguém lê a saída padrão de um serviço, e um serviço que falha sem deixar
rastro não tem como ser diagnosticado. Em primeiro plano e no Linux (onde o `journald` guarda a
saída), segue para a saída padrão, com cor só num terminal de verdade.

**4. O balanço do PROGRESSO.** Uma leitura item a item atrás do que estava feito e desmarcado, ou
marcado e desatualizado:

- *Configuração com escrita atômica*: já era — arquivo temporário e `rename`, na configuração e na
  identidade, com `0600` no Linux. Marcado.
- *Diagnóstico por campos permitidos*: o relatório do serviço é um formato fechado (papel, fase,
  enlace, quantos pares, endereço do par), sem nada do que foi digitado. Marcado.
- *Caminhos de sistema*: **em andamento**, não feito. O estado vai para `%ProgramData%\InputRemote`
  e `/var/lib/inputremote`, mas não na divisão de [02, §7](../02-arquitetura.md): a configuração do
  Linux deveria estar em `/etc/inputremote`, e identidade e pares do Windows numa subpasta `state\`.
  Mudar isso exige migrar as instalações que já existem, e ficou para depois.
- *PoC-5* (`Noise_XX` com código de seis dígitos, `Noise_IK` com chave fixada, janela de repetição):
  a prova de conceito nunca rodou à parte porque o produto já os tem desde a Etapa 3
  ([log 13](13-pilha-completa-mouse-cruzando.md)). Marcados, apontando para lá.
- *PoC-6, um comando gera tudo*: gera o MSI e o RPM; faltam ZIP e DEB. Em andamento.
- Um item **duplicado** e em aberto sobre a troca de papel que só valia no reinício — fechado no
  [log 19](19-a-troca-que-vale-na-hora.md) — foi removido.
- A nota da Etapa 6 dizia que o `uinput` nunca tinha rodado num Linux com ambiente gráfico. Rodou: os
  dois dispositivos aparecem no notebook ([log 18](18-a-prova-no-notebook-e-a-troca-de-papel.md)). O
  que falta é vê-los mexer o ponteiro com o par conectado.
- A nota da Etapa 9 dizia que a janela cai para o simulado sem o serviço. Não cai mais: diz o motivo
  e reconecta sozinha ([log 17](17-a-janela-que-volta-e-o-grupo-que-vale-na-hora.md)).

## Arquivos

- `empacotar/windows/Produto.wxs` — `MSIRESTARTMANAGERCONTROL`
- `crates/ir-daemon/src/service.rs` — `StopPending`, canal de parada, prazo
- `crates/ir-daemon/src/main.rs` — canal de parada até o ator; registro por fila e em arquivo
- `crates/ir-daemon/src/actor/parada.rs` (novo), `actor/bancada.rs` (novo, só em teste),
  `actor/mod.rs`, `actor/partes.rs`, `actor/papel.rs`
- `Cargo.toml`, `crates/ir-daemon/Cargo.toml` — `tracing-appender`
- `PROGRESSO.md`, `LOG.md`

## Verificação

- **Windows:** 457 testes; `clippy --all-targets -D warnings`, `fmt --check` e `cargo xtask`
  (134 arquivos) limpos.
- **Linux, no container Fedora 44:** 166 testes em `ir-input`, `ir-ipc`, `ir-session` e
  `ir-daemon`, entre eles os dois de parada; clippy limpo.
- **O instalador:** `candle` e `light` compilados numa pasta à parte, e a base do MSI lida pela API
  do Windows Installer: `MSIRESTARTMANAGERCONTROL = Disable`, e o `ServiceControl` do serviço com
  evento 163 (inicia na instalação; para na instalação e na remoção; remove na remoção).
  **Compilar pegou um erro**: o comentário novo tinha `--`, que XML não aceita em comentário, e o
  MSI não sairia. Corrigido antes de qualquer instalação.

**O que não foi verificado:** instalar por cima e ver que não pede reinício; a parada limpa num
SCM de verdade; o arquivo de registro aparecendo em `logs\`. Isso depende de instalar, e a
instalação antiga ficou aberta na tela do aviso, travando o `dist\` — o MSI definitivo só pode ser
gerado depois de ela ser cancelada. Se ainda pedir reinício, parar o serviço à mão antes
(`sc stop InputRemote`, como administrador) garante que o agente saiu.

## O que fica para depois

O que foi deliberadamente deixado, em ordem de quanto destrava:

1. **O teste físico completo** — parear Windows e Linux, cruzar o ponteiro, digitar do outro lado.
   É o que prova o produto; tudo abaixo é aprimoramento.
2. **Captura no Linux** (portal `InputCapture`/`libei`) — hoje o Linux só pode ser o controlado.
3. **Tela de bloqueio e desktop seguro** (níveis N2/N3).
4. **Portador Bluetooth**, área de transferência e arquivos, e a troca de chaves periódica.
5. **Endurecimento do Linux** — usuário dedicado, regra `udev`, política D-Bus — e os caminhos de
   sistema na divisão da especificação.
6. **Distribuição** — assinatura de verdade (Authenticode e GPG), ZIP e DEB.
