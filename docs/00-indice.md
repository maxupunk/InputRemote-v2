# Índice da documentação

A ordem abaixo é a ordem de leitura. Cada documento pressupõe o anterior.

| # | Documento | O que decide |
|---|---|---|
| 00 | [Lições do v1](00-licoes-do-v1.md) | o que não repetir, com evidência |
| 00b | [Lições do Deskflow](00-licoes-do-deskflow.md) | o que aprender de quem já resolveu isto |
| 01 | [Visão e escopo](01-visao-e-escopo.md) | o que o produto é, o que ele não é |
| 02 | [Arquitetura](02-arquitetura.md) | processos, crates, fronteiras |
| 03 | [Protocolo](03-protocolo.md) | camadas, canais, formato dos quadros |
| 04 | [Segurança](04-seguranca.md) | pareamento, cripto, privilégio, modelo de ameaça |
| 05 | [Plataforma Windows](05-windows.md) | serviço, agente, desktops, tela de bloqueio |
| 06 | [Plataforma Linux](06-linux.md) | evdev, uinput, portais, greeter |
| 07 | [Stack e dependências](07-stack-e-dependencias.md) | linguagem, libs, e por quê |
| 08 | [Plano de implementação](08-plano-de-implementacao.md) | etapas, provas de conceito, ordem |
| 09 | [Padrões de código](09-padroes-de-codigo.md) | regras que o CI faz cumprir |
| 10 | [Testes e validação](10-testes-e-validacao.md) | como se prova que funciona |
| 11 | [Nada de legado](11-nao-legado.md) | o que está proibido, e o que é antigo mas correto |

## Decisões arquiteturais (ADR)

Cada ADR registra uma decisão, as alternativas descartadas e o custo aceito.
Uma decisão só muda por um ADR novo que substitua o anterior — não por edição.

| ADR | Decisão |
|---|---|
| [0001](adr/0001-tres-processos.md) | Três processos: serviço, agente, interface |
| [0002](adr/0002-rust.md) | Rust como linguagem do núcleo |
| [0003](adr/0003-noise-em-vez-de-quic.md) | Noise sobre UDP/TCP/RFCOMM, em vez de QUIC |
| [0004](adr/0004-nucleo-sans-io.md) | Núcleo de sessão sem E/S |
| [0005](adr/0005-bluetooth-rfcomm-winsock.md) | RFCOMM via Winsock, e pareamento do SO manual |
| [0006](adr/0006-entrada-linux-evdev-uinput.md) | evdev + uinput como caminho primário no Linux |
| [0007](adr/0007-ui-slint-processo-separado.md) | Interface em Slint, em processo separado |
| [0008](adr/0008-agente-com-thread-por-desktop.md) | Um agente por sessão, com uma thread por desktop |
| [0009](adr/0009-canal-rfcomm-fixo-sem-sdp.md) | Canal RFCOMM fixo, sem SDP |
| [0010](adr/0010-canal-de-dados-em-tcp-proprio.md) | O canal de dados em TCP próprio, fora da abstração de portador |
| [0011](adr/0011-clipboard-na-travessia.md) | O clipboard sincroniza na travessia, por um ajudante que roda como o usuário |
| [0012](adr/0012-rota-dupla.md) | Rota dupla: Bluetooth e rede ao mesmo tempo, vale o que chegar primeiro |
| [0013](adr/0013-economia-de-energia-do-wifi.md) | Avisar da economia de energia do Wi-Fi, e desligá-la com um botão — também no par |

## Registro do que foi feito

Documento diz o que o produto **deve** ser. Estes dois dizem o que ele **é** hoje.

| Arquivo | Para quê |
|---|---|
| [PROGRESSO.md](../PROGRESSO.md) | checklist verificável; `[x]` só com verificação registrada |
| [LOG.md](../LOG.md) | índice das entradas de [`logs/`](logs/), um arquivo por tópico |

As entradas de log não são editadas depois de escritas. Correção vira entrada nova, dizendo o
que mudou e por quê — um registro que se reescreve não é registro.

## Convenções

- **Servidor**: o computador que tem o teclado e o mouse físicos.
- **Cliente**: o computador controlado.
- **Serviço**: `inputremote-daemon`, privilegiado, sobe com a máquina.
- **Agente**: `inputremote-agent`, roda dentro de uma sessão/desktop gráfico.
- **Interface**: `inputremote-ui`, sem privilégio, aberta sob demanda.

Um requisito escrito como **DEVE** é obrigatório; **NÃO DEVE** é proibido; **PODE** é
opcional. Requisitos sem essas palavras são contexto, não contrato.
