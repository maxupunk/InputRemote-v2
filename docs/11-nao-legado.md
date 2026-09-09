# 11 — Nada de legado

Regra do projeto: **não se herda nada do InputRemote 1, e não se adota tecnologia morta ou
em fim de vida.** Este documento é a lista fechada. Uma exceção só entra por ADR.

Há uma distinção que precisa ficar clara antes da lista, porque é onde as discussões sobre
"legado" costumam se perder:

> **Antigo não é legado.** Legado é o que foi substituído por algo melhor e continua no
> código por inércia. Uma API de 2005 que é a única suportada, ainda mantida pelo
> fabricante e sem sucessora aplicável ao nosso caso não é legado — é a API.

As duas listas abaixo separam exatamente isso.

## 1. Excluído por ser legado

### Do InputRemote 1

| Excluído | Substituto |
|---|---|
| Todo o código do v1 — nenhuma linha é reaproveitada | reescrita a partir da especificação |
| `controller.rs` / `application.rs` e o modelo de crate único de GUI | [ADR-0004](adr/0004-nucleo-sans-io.md), limites de tamanho em [09](09-padroes-de-codigo.md) |
| Processo único | [ADR-0001](adr/0001-tres-processos.md) |
| `eframe` / `egui` | Slint em processo separado, [ADR-0007](adr/0007-ui-slint-processo-separado.md) |
| QUIC (`quinn`, `rustls`, `rcgen`, `tokio-rustls`) | Noise, [ADR-0003](adr/0003-noise-em-vez-de-quic.md) |
| `spake2` (versão pré-lançamento no caminho de segurança) | Noise_XX + código visual, [04, §3.2](04-seguranca.md) |
| WinRT (`Windows.Devices.Bluetooth`) | Winsock `AF_BTH`, [ADR-0005](adr/0005-bluetooth-rfcomm-winsock.md) |
| Modo "Híbrido" com três políticas de degradação | política única de portador, [01, §5](01-visao-e-escopo.md) |
| Pareamento Bluetooth dirigido pelo aplicativo | pareamento do SO, manual, [ADR-0005](adr/0005-bluetooth-rfcomm-winsock.md) |
| Documento único de 53 KB | documentos por assunto + ADRs |

### Do Windows

| Excluído | Por quê | Usamos |
|---|---|---|
| `keybd_event`, `mouse_event` | substituídas pelo próprio fabricante | `SendInput` |
| Ganchos por injeção de DLL (`WH_KEYBOARD`, `WH_MOUSE`, `WH_GETMESSAGE` em DLL) | anteriores aos ganchos de baixo nível; injetar DLL em processo alheio é comportamento de ataque. O Deskflow ainda carrega esse caminho ([00b, §7](00-licoes-do-deskflow.md)) | `WH_KEYBOARD_LL`, `WH_MOUSE_LL`, Raw Input |
| GINA | morta desde o Vista | provedores de credencial não são usados; injetamos entrada |
| Serviços interativos (`SERVICE_INTERACTIVE_PROCESS`) | desligados desde o Vista pelo isolamento da sessão 0 | serviço + agente, [ADR-0001](adr/0001-tres-processos.md) |
| `GetAsyncKeyState` para reconstruir estado de teclado | corrida por natureza | `StateSnapshot` no protocolo, [03, §7](03-protocolo.md) |
| Injeção de movimento relativo do ponteiro | sofre a balística do sistema duas vezes | injeção absoluta, [05, §4.2](05-windows.md) |

### Do Linux

| Excluído | Por quê |
|---|---|
| X11, `XTEST`, `XRecord` | o alvo é Wayland; X11 já é bem servido por outras ferramentas |
| `xdotool` e afins | dependem de X11 |
| Backends de clipboard baseados em X | idem |
| Chamar `ydotool` como processo externo | dependência de processo de terceiro no caminho de entrada; falamos com `uinput` diretamente |

### Gerais

| Excluído | Por quê |
|---|---|
| Compatibilidade de protocolo com Synergy, Barrier ou Deskflow | protocolo texto de 2001, com vinte anos de decisões que não são nossas |
| Suporte a Windows anterior ao 10 22H2 | fora de suporte do fabricante |
| Suporte a X11 e a compositores sem `libei` para o papel de servidor | [ADR-0006](adr/0006-entrada-linux-evdev-uinput.md) |
| Atualizador automático | superfície de ataque num serviço privilegiado |
| Telemetria, contas na nuvem, relay pela internet | não existem no produto |
| Instalação por script baixado da internet | instalador assinado ou pacote da distribuição |

## 2. Antigo, mas correto — escolhas conscientes

Estas escolhas parecem legado e não são. Cada uma está aqui porque é a única opção que
atende o requisito, e a alternativa "moderna" foi avaliada e reprovada. Se alguma delas
for questionada no futuro, a resposta está aqui.

### Bluetooth Classic / RFCOMM por Winsock `AF_BTH`

*Parece legado:* API de sockets de Bluetooth de 2005, perfil BR/EDR, sem WinRT.

*Por que fica:* é a única que funciona a partir de um serviço na sessão 0, que é o
requisito R1. As alternativas modernas foram avaliadas:

| Alternativa | Reprovada porque |
|---|---|
| WinRT `Windows.Devices.Bluetooth` | não é alvo suportado para serviço na sessão 0 |
| BLE / GATT | exige que um dos adaptadores atue como periférico, o que não é garantido em PC com Windows; MTU pequena; intervalo de conexão impõe piso de latência |
| HID over GATT | o Windows não publica o serviço HID (`0x1812`) como periférico, e o servidor é Windows |

RFCOMM não está descontinuado: é plenamente suportado no Windows 11 e no BlueZ atuais.
Ver [ADR-0005](adr/0005-bluetooth-rfcomm-winsock.md).

### Scancodes PS/2 na injeção de teclado

*Parece legado:* conjunto de scancodes de teclado AT.

*Por que fica:* `SendInput` com `KEYEVENTF_SCANCODE` é o único caminho que deixa o layout
do computador **controlado** decidir o caractere — requisito direto da tela de login. O
protocolo em si usa HID Usage IDs, que é o padrão moderno; o scancode aparece só na última
tradução, dentro do backend. Ver [03, §5](03-protocolo.md).

### Ganchos `WH_KEYBOARD_LL` / `WH_MOUSE_LL`

*Parece legado:* API de ganchos globais.

*Por que fica:* é o único mecanismo do Windows que permite **suprimir** a entrada local.
Raw Input dá deltas melhores mas não suprime. Usamos os dois, cada um para o que faz bem.
Ver [05, §5](05-windows.md).

### `uinput` e `evdev`

*Parece legado:* interface de dispositivo de entrada do kernel, de aparência antiga.

*Por que fica:* não é legado de forma alguma — são as interfaces atuais e mantidas do
kernel Linux, e a base sobre a qual `libinput` e todos os compositores funcionam. É
justamente por estar abaixo do compositor que atravessa greeter e tela de bloqueio.

## 3. Como esta regra é mantida

- toda dependência nova entra por [07](07-stack-e-dependencias.md), com justificativa;
- toda exceção às listas acima entra por ADR, nunca por commit isolado;
- `cargo deny` recusa dependências não mantidas ou com aviso do RustSec;
- nenhuma API marcada como *deprecated* pelo fabricante é chamada; se for inevitável, vira
  ADR com data de reavaliação.
