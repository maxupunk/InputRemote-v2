# O Wi-Fi que cochilava, o botão que o acorda, e o rádio que não reabria depois de atualizar

**Data:** 2026-09-22

**Itens:** Etapa 7 — qualidade da entrada pela rede; [ADR-0013](../adr/0013-economia-de-energia-do-wifi.md).

**O que foi relatado:** mexendo o mouse depois de desligar o Bluetooth, ele fluía por um trecho,
travava, e voltava a fluir. Acontecia também com o Deskflow, e nunca pelo Bluetooth. Com o Bluetooth
religado continuava, com menos impacto. O pedido: ver se é a economia de energia das placas de rede,
mostrar um aviso, e um botão para resolver.

## A medida

Ping a cada segundo, do Windows:

| Para | Resultado (ms) |
|---|---|
| o roteador | 5 4 1 1 1 1 1 … 7 1 2 8 |
| o Fedora, economia ligada | 2 1 1 28 1 4 2 8 2 45 99 **190** 6 28 1 67 1 2 2 **126** |
| o Fedora, `iw … power_save off` | 1 1 12 1 1 2 2 1 … 7 1 5 6 2 1 6 |

O Fedora (Realtek RTL8852BE, driver `rtw89`) estava com `Power save: on`; o Windows (Intel AX211)
com "desempenho máximo" na tomada e "economia média" na bateria. Com a economia ligada, a placa
cochila entre pacotes e o ponto de acesso guarda o que chega até o próximo *beacon*.

O detalhe que explica o "flui e trava **com o mouse em movimento**": esse driver decide se acorda
pela **vazão**, e os comandos de entrada são pequenos demais para contar. Nada que o produto mande
no tráfego a mantém acordada; a correção é a configuração da placa. E é por isso que o Bluetooth
religado só **diminui** o impacto: na rota dupla ele cobre os buracos do Wi-Fi, mas com a latência
dele, que é maior que a do Wi-Fi acordado.

## O que foi feito

- **`ir-energia`**, crate de plataforma novo: lê e desliga a economia do Wi-Fi. No Linux, `iw` agora e
  `/etc/NetworkManager/conf.d/90-inputremote-wifi.conf` (`wifi.powersave=2`) para sobreviver a
  reconexões; no Windows, o "modo de economia de energia" do adaptador sem fio no plano ativo. A
  leitura do `powercfg` não lê texto — ele muda de idioma —, lê os dois índices hexadecimais.
- **Protocolo 3, mínimo 2**: `Control::NetworkPower` e `Control::DisableNetworkPowerSaving`, que só
  vão para um par da versão 3. A versão 2 continua conversando.
- **O serviço** verifica a placa na subida e a cada 30 s, conta ao par, e desliga quando pedem — pela
  janela daqui ou pelo par. Toda verificação é repetida ao par, e não só as mudanças (ver abaixo).
- **A janela** mostra o aviso com o botão **Resolver** na tela inicial. O aviso do par vem primeiro:
  quem olha a tela é quem sente o mouse travar do outro lado.
- O RPM passa a exigir `iw`.

## O que a bancada mostrou

Com a versão nova instalada nas duas máquinas, o usuário clicou nos botões antes de a verificação
automática terminar:

| Hora (UTC) | O quê |
|---|---|
| 00:08:50 | **Resolver** na janela do Fedora: `economia … desligada`; `iw` diz `Power save: off`, e o arquivo do `NetworkManager` foi gravado |
| 00:12:17 | Windows sobe e se lê: `economia=SoNaBateria` |
| 00:12:55 | **Resolver** na janela do Fedora, agora para o aviso do Windows: `o par pediu para desligar a economia de energia do Wi-Fi daqui`, e o Windows foi a `Desligada` — `powercfg` passou de `0x0 / 0x2` para `0x0 / 0x0` |
| depois | Ping Windows → Fedora, 30 amostras: pior caso **6 ms** (antes, 176) |
| 00:23:29 | Economia religada à mão no Fedora; 19 s depois o serviço de lá leu `Ligada`, e o Windows publicou `no par=Some(Ligada)` com o aviso "O Wi-Fi do outro computador está economizando energia…" |

Com a versão final instalada nas duas máquinas, o outro sentido e a persistência:

| Hora (UTC) | O quê |
|---|---|
| 00:50:50 | Windows sobe; o rádio abre, a rota fica `bluetooth+udp`, e o Windows registra `economia de energia do Wi-Fi do par estado=On` já no estabelecimento da sessão |
| 00:54:50 | Placar do Windows: `rede ouvido há 42761 ms` — 42 s sem nada pela rede, com a economia do Fedora ligada; a sessão seguiu pelo Bluetooth |
| 00:55:50 | O pedido do botão **Resolver** do Windows (pela ferramenta de bancada, que manda o mesmo pedido): no Fedora, `o par pediu para desligar a economia de energia do Wi-Fi daqui`, `desligada`, `iw` em `off`, o arquivo do `NetworkManager` gravado; no Windows, o aviso some |
| depois | Ping Windows → Fedora, 30 amostras: pior caso **3 ms** |
| 00:58:12 | `nmcli connection up` no Fedora — a reconexão que antes religaria a economia. Terminou em `connected` com **`Power save: off`**: o arquivo do `NetworkManager` vale. Nenhuma sessão encerrada no Windows |

O rádio que não reabria (abaixo) não se repetiu nesta instalação — o canal estava livre na subida —,
então a nova tentativa em segundo plano está coberta pelo código e pelo registro, mas não foi vista
agindo no hardware.

## Três defeitos que apareceram no caminho

**A mudança que não chegou.** Às 00:19:29 o Fedora leu `Ligada`, e o Windows não mostrou; às 00:23 o
mesmo caminho funcionou. A mensagem vai pelo canal confiável e nenhum dos dois lados registrou
queda — sem registro do lado que recebe, não deu para provar onde se perdeu. Duas mudanças: o Windows
passa a registrar a economia do par quando ela muda, e **toda** verificação é repetida ao par (uma
mensagem de poucos bytes a cada 30 s), então o que o par sabe converge sozinho em até 30 s.

**O rádio que não reabria depois de atualizar.** Numa atualização, o serviço novo subiu 4 s depois de
o antigo parar, e o Windows ainda não tinha liberado o canal RFCOMM 23 (`os error 10048`). O serviço
desistia do rádio até a próxima reinicialização, e a rota dupla ficava só na rede. Agora, quando o
rádio existe mas o canal está ocupado, a subida tenta de novo em segundo plano a cada 3 s por até
2 min, e o rádio que abrir chega ao ator — que anuncia o endereço e disca como se tivesse aberto na
subida. A rede sobe na hora, como antes.

**As frases com um buraco no meio.** "a placa cochila                entre pacotes": a continuação de
linha (`\` no fim) das frases longas se perdeu em edições feitas por um script que gravava CRLF. A
mesma coisa já estava na explicação de endereço inválido da janela de pareamento, desde antes deste
trabalho. As seis foram corrigidas, e dois testes passam a recusar espaço duplo nessas frases.

## Fronteiras que o limite apontou

- **`ir-canais`**: o servidor de IPC (controle e agente) saiu do `ir-daemon`, que estava no teto de
  tamanho. Ele só conhecia o vocabulário do `ir-ipc`, o portão do `ir-acesso` e o `tokio`. O
  `ir-daemon` foi de 2 495 para 1 941 linhas de produção, e o aviso antigo de `motivo` nunca lido
  (`controle.rs`) foi corrigido na mudança.
- O `run` do ator passou de 60 linhas com o terceiro canal de fundo. Busca na rede, verificação de
  energia e rádio tardio viraram um enum só, `DeFundo`, num canal só.
- Na janela, a ligação dos botões aos pedidos foi para `janela/acoes.rs`, e os testes da ponte para
  `ponte/testes.rs`.
