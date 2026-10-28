# O Bluetooth que travou

**Data:** 2026-10-28 · **Branch:** `feat/painel-android`

**Itens:** [PROGRESSO](../../PROGRESSO.md), rota dupla ([ADR-0012](../adr/0012-rota-dupla.md)).

**O que foi pedido:** "vamos verificar por que estou com dificuldades de conectar esse computador
ao notebook fedora. Ao achar o problema vamos resolver de forma que tenha uma ótima experiência de
usuário." Na escolha de como recuperar, a pessoa ficou com **automático com salvaguarda**.

## O que havia

O registro do Fedora contou a história:

- Até 11:28 a rota dupla estava de pé (Bluetooth e rede), com cópias e travessias normais.
- Às 11:28 o enlace Bluetooth caiu com "o rádio parou de escoar". Dali em diante, a cada discagem:
  ~20 s de "ocupado" e depois, sem parar, "o par não atendeu no canal do InputRemote — Verifique se
  o InputRemote está em execução no outro computador" seguido de `os error 38` (`ENOSYS`).
- A rede seguiu de pé o tempo todo: o outro computador **estava** em execução. A frase era falsa.
- Às 11:46 a pessoa pausou, esqueceu o par e tentou parear de novo — pelo Bluetooth, que falhou; e
  depois pela rede, cujo código venceu em 2 min sem confirmação.

O `ENOSYS` não era do InputRemote: o `bluetoothd` do próprio Fedora recebia o mesmo erro ao tentar o
áudio (A2DP) com o Windows desde 11:29. O kernel devolve `ENOSYS` quando a controladora responde com
um código HCI que ele não mapeia (provavelmente `0x22`, *LMP Response Timeout*; sem `btmon` — falta
root na bancada —, fica como provável). Uma sonda RFCOMM do Fedora ao Windows dava **timeout** até em
canal vazio: o rádio nem alcançava o outro. Depois de `bluetoothctl power off` / `on`, a mesma sonda
recebeu "recusado" em 1 s num canal vazio e **conectou em 30 ms** no canal 23. A controladora
(Realtek RTL8852BU, combo de Wi-Fi e Bluetooth) travou, e só religar o rádio a soltou.

## O que mudou

- **Crate novo `ir-religamento`** (o `ir-daemon` está no teto de 2 500 linhas):
  - `Vigia`: recebe a cada batida os enlaces (`radio_esperado`, `radio`, `rede`) e a hora. Com o
    Bluetooth fora **e a rede de pé** há `PRAZO` (60 s), religa o rádio daqui — uma vez só por
    queda. A rede é a prova de que o par está ligado e com o serviço no ar; o prazo deixa passar a
    queda comum, que volta em ~20 s (log 28).
  - Salvaguarda: com outro aparelho Bluetooth conectado nesta máquina (ou sem dar para saber), não
    religa sozinho — o aviso oferece o botão e diz o custo.
  - Não volta → "não resolveu", com o botão para tentar de novo. O Bluetooth de volta zera tudo.
  - Por tempo, e não por erro de discagem: só um dos lados disca (`turno`), e o outro também
    precisa perceber.
  - `Adaptador` (trait): no Linux, `bluetoothctl` pelo `ir_processo::ferramenta` (o mesmo corredor
    do `iw`): o `bluetoothd` fica sabendo, sem `unsafe` e sem D-Bus no RPM. No Windows ainda não
    religa: o aviso ensina o caminho do sistema.
- **Contrato** (`ir-ipc`): `Estado.bluetooth_parado` (`Religando`, `ComOutrosAparelhos`,
  `NaoResolveu`, `SemReligar`), `Pedido::ReligarBluetooth` (fim do enum, `Configurar`), e
  `AcaoDoAviso`: o botão da faixa deixou de ser "o do Wi-Fi" (`resolver_no_par`) e passou a ser uma
  ação com pedido, rótulo e nota. A janela manda `acao.pedido()` — um aviso novo com botão não
  precisa de `callback` novo no `.slint`.
- **Janela**: a faixa mostra rótulo e nota vindos do contrato (`aviso-botao`, `aviso-nota`,
  `resolver-aviso()`). Ordem dos avisos: o do Wi-Fi vem antes do Bluetooth parado.
- **Daemon**: só a cola — `vigiar_o_radio()` dentro de `notar_estado()`, o campo, o pedido e o campo
  do estado. Os embrulhos `economia_*_na_tela` saíram (repassavam para `ir_painel`).

## O que foi provado

- 11 testes do vigia (relógio de mentira, adaptador falso): soluço que volta antes do prazo, rádio
  travado religado sozinho, uma vez só por queda, outro aparelho conectado, sem saber quem está no
  rádio, sem rede, sem suporte, sistema que recusa, mudança contada uma vez.
- Testes do contrato e da janela para o aviso e o botão novos.
- `clippy -D warnings` no workspace (Windows) e nos crates novos no Fedora; `xtask check`.
- No Fedora, o adaptador real: `aparelhos conectados: Ok(0)`, `religar: Ok(())` em 2,3 s, rádio de
  volta ligado e o canal 23 do Windows alcançável em seguida.

## O que falta

- Bancada com o serviço instalado nos dois lados (exige administrador): parear de novo — os dois
  estão sem par desde 11:46 —, desligar o rádio do Fedora com `bluetoothctl power off` no meio de uma
  sessão e ver, em ~1 min, a faixa "Religando" e o Bluetooth voltar sozinho.
- Windows: religar pelo serviço (`pnputil /restart-device` na controladora, como SYSTEM) — hoje o
  aviso só ensina o caminho do sistema.
- A pareação que sai pelo Bluetooth quebrado: com a rede alcançando o mesmo computador, "Procurar"
  poderia preferi-la.
