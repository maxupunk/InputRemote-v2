# 06 — Plataforma Linux (Wayland)

## 1. O problema, em uma frase

No Wayland, captura e injeção de entrada passam por portais, que negociam com o
compositor e com o usuário. Só que na tela de login **não existe usuário, não existe
sessão e não existe portal** — o GDM roda seu próprio compositor, isolado. O caminho
correto para o uso normal é justamente o caminho indisponível para o requisito R1.

A resposta é assimétrica, e é assimétrica de propósito:

| Papel | Caminho | Por quê |
|---|---|---|
| **Cliente** (injeta, precisa da tela de bloqueio) | `/dev/uinput`, direto do serviço de sistema | funciona em qualquer compositor, no greeter, na tela de bloqueio e no console |
| **Servidor** (captura, sempre em sessão desbloqueada) | portal `InputCapture` + `libei` | é a API feita para isto, e a única que sabe dizer "o ponteiro encostou na borda" |

O servidor é, por definição, a máquina que você está usando neste instante — ela está
desbloqueada. O requisito da tela de bloqueio vale só para o cliente. Cada lado usa a
tecnologia em que é forte. Ver [ADR-0006](adr/0006-entrada-linux-evdev-uinput.md).

## 2. Injeção: `uinput`, no serviço

### 2.1. Dispositivos virtuais

O serviço cria **três** dispositivos na subida, e os mantém pelo tempo de vida do processo:

| Dispositivo | Capacidades |
|---|---|
| `InputRemote Keyboard` | `EV_KEY` para toda a faixa `KEY_*` que o mapa HID cobre, `EV_LED`, `EV_REP` desligado |
| `InputRemote Pointer` | `EV_REL` (`REL_X`, `REL_Y`, `REL_WHEEL_HI_RES`, `REL_HWHEEL_HI_RES`), `EV_KEY` (`BTN_LEFT`…`BTN_EXTRA`) |
| `InputRemote Tablet` | `EV_ABS` (`ABS_X`, `ABS_Y` em `0..65535`), `BTN_TOOL_PEN`, `INPUT_PROP_DIRECT` |

O terceiro existe para posicionamento **absoluto**, que é como o ponteiro é injetado —
mesmo motivo do Windows ([05, §4.2](05-windows.md)): o compositor aplica aceleração a
movimento relativo, e o servidor já entregou deltas acelerados. Injetar absoluto evita
aceleração aplicada duas vezes.

### 2.2. A armadilha do `UI_DEV_CREATE`

Depois de `UI_DEV_CREATE`, o `udev` precisa processar o dispositivo e o compositor
precisa abri-lo pelo `libinput`. Isso leva de centenas de milissegundos a mais de um
segundo. **Eventos enviados nessa janela são perdidos em silêncio.**

Regra: os dispositivos são criados na subida do serviço, **nunca** no momento de usar.
O serviço só declara o caminho de injeção pronto após confirmar, pelo `libudev`, que o
dispositivo apareceu e recebeu as marcas `ID_INPUT_KEYBOARD` / `ID_INPUT_MOUSE`.

Este é o defeito número um de quem usa `uinput`: cria o dispositivo, injeta a primeira
tecla, nada acontece, e o problema some quando se coloca um `sleep` no meio.

### 2.3. Onde funciona

`uinput` entra abaixo do compositor, no nível do kernel. Portanto funciona no greeter do
GDM, na tela de bloqueio do GNOME, em prompts do `polkit`, em qualquer compositor Wayland,
no X11 e no console de texto. É a mesma técnica do `ydotool`, e é a única que atravessa
todos esses contextos.

O preço é honesto e precisa estar escrito: isto **contorna** o modelo de segurança de
entrada do Wayland. É aceitável porque o usuário instalou deliberadamente um serviço
privilegiado para exatamente este fim — e o controle de quem pode pedir injeção está
em [04, §5](04-seguranca.md), não no compositor.

### 2.4. Permissões

O acesso a `/dev/uinput` é governado por **permissão de arquivo**, não por capacidade do
kernel. Portanto o serviço **não precisa ser `root`**: ele roda como o usuário de sistema
`inputremote`, e o pacote instala a regra `udev` que dá acesso ao grupo:

```udev
KERNEL=="uinput", MODE="0660", GROUP="inputremote", OPTIONS+="static_node=uinput"
SUBSYSTEM=="input", ATTRS{name}=="InputRemote *", ENV{ID_INPUT}="1"
```

`static_node=uinput` garante o nó presente mesmo antes de o módulo ser carregado sob
demanda — sem isso, o serviço pode subir cedo demais no boot e não encontrar o dispositivo.

A confirmação de que esse usuário também consegue registrar o perfil no BlueZ é parte da
PoC-3. Se não conseguir, o serviço sobe como `root` e larga o privilégio depois de abrir
`/dev/uinput` e o barramento — decidido pela PoC, não por suposição
([04, §4](04-seguranca.md)).

## 3. Captura: portal `InputCapture` + `libei`

### 3.1. Por que este caminho

O `InputCapture` é o único mecanismo que resolve o problema real da captura num KVM:
**saber que o ponteiro encostou na borda da tela**. Ele instala barreiras no compositor,
avisa quando são cruzadas e entrega os eventos por `libei`. Com `evdev` cru isso é
impossível — o compositor não conta onde está o cursor, e reconstruir a posição por
acumulação de deltas diverge do cursor real por causa da aceleração.

Estado em setembro de 2026: as integrações de `libei` nos portais `RemoteDesktop` e
`InputCapture` estão no `xdg-desktop-portal` desde a versão 1.21.0 e nos compositores
principais, com o caminho `ConnectToEIS()` validado contra uma sessão GNOME Wayland real
em 01/09/2026. Não é mais território experimental, como era quando o v1 foi escrito.

### 3.2. Fluxo

```text
1. agente (na sessão do usuário) → CreateSession no org.freedesktop.portal.InputCapture
2. GetZones                → geometria das telas, para posicionar barreiras
3. SetPointerBarriers      → uma barreira na borda que dá para o computador par
4. ConnectToEIS()          → descritor de arquivo para o contexto libei
5. Enable                  → a captura fica armada
6. sinal Activated         → o ponteiro cruzou; a partir daqui os eventos chegam por libei
7. Disable / Release       → o controle volta para a máquina local
```

O `restore_token` devolvido pelo portal **DEVE** ser persistido e reapresentado. Sem
isso, o usuário vê um diálogo de permissão a cada reconexão — o item que ficou aberto no
v1 (*"Persistência e renovação do `restore_token` — [ ]"*) e a diferença entre um produto
usável e um irritante.

### 3.3. Quando `InputCapture` não existe

Compositores sem `InputCapture` v1/v2 (vários baseados em wlroots) não têm caminho de
servidor. A resposta, em ordem:

1. dizer isso na interface, com o nome do compositor detectado e o que falta;
2. a máquina continua servindo como **cliente** normalmente — injeção por `uinput` não
   depende de portal nenhum;
3. **Fase 2:** modo `evdev` exclusivo, em que o serviço faz `EVIOCGRAB` nos dispositivos
   físicos e passa a alimentar o compositor por um dispositivo `uinput` absoluto próprio.
   Assim ele conhece a posição do cursor porque é ele quem a produz. O custo é
   implementar a curva de aceleração do ponteiro local, e por isso não entra na Fase 1.

## 4. Onde a entrada é roteada

O serviço decide o alvo consultando o `logind` (`org.freedesktop.login1`):

| Situação detectada | Injeção |
|---|---|
| Sessão ativa de classe `user`, desbloqueada | `uinput` |
| Sessão ativa de classe `user`, bloqueada | `uinput` |
| Sessão ativa de classe `greeter` (GDM, SDDM) | `uinput` |
| Nenhuma sessão gráfica (console, boot) | `uinput` |

Ou seja: sempre `uinput`. A consulta ao `logind` não escolhe o método — ela alimenta a
interface e o diagnóstico, e habilita a política de tela de bloqueio de
[04, §6](04-seguranca.md), que precisa saber se a máquina está bloqueada.

Isso é uma simplificação deliberada em relação ao v1, que mantinha dois caminhos de
injeção vivos (portal e `uinput`) com regras de escolha entre eles.

### 4.1. Níveis N3 e N2 no Linux

Os níveis estão definidos em [01, §2](01-visao-e-escopo.md). No Linux a situação é mais
favorável que no Windows, e por um motivo estrutural: `uinput` entra **abaixo** do
compositor, então tela de bloqueio e greeter não são casos diferentes de injeção — são o
mesmo caso.

| | N2 — tela de bloqueio | N3 — greeter (GDM/SDDM) |
|---|---|---|
| Onde roda | dentro da sessão do usuário (o GNOME desbloqueia na própria sessão) | sessão separada, do usuário `gdm`, com compositor próprio |
| Caminho de injeção | `uinput` | `uinput`, idêntico |
| O que pode falhar | nada específico | ver abaixo |

Riscos específicos de N3, e as respostas:

| Risco | Resposta |
|---|---|
| Serviço ainda não subiu quando o greeter aparece | `After=bluetooth.target network.target`, `WantedBy=multi-user.target` — anterior ao `graphical.target`; `Type=notify` só sinaliza pronto com os dispositivos confirmados |
| Módulo `uinput` não carregado tão cedo no boot | `OPTIONS+="static_node=uinput"` na regra `udev` (§2.4) |
| Dispositivos virtuais não atribuídos ao `seat0` do greeter | dispositivo `uinput` sem marca de assento cai no `seat0` por padrão; conferido na PoC-3 |
| Rede sem endereço e rádio ainda inicializando | mesmo tratamento do Windows: pronto só com portador disponível, e último endereço conhecido tentado primeiro |
| Tela apagada por DPMS | a primeira tecla acorda o monitor; contabilizar isso na medição, não confundir com falha |

**Se N3 não for alcançável no Linux**, vale a mesma regra do Windows: entrega-se N2, com a
limitação declarada, e o lançamento não é bloqueado. Mas a expectativa honesta é que o
Linux alcance N3 com mais facilidade que o Windows, porque não há nada equivalente ao
endurecimento de credencial de janeiro de 2026 nem ao desktop seguro — a barreira do
Wayland é do compositor, e `uinput` está abaixo dela.

## 5. Agente de sessão

O agente Linux é bem menor que o do Windows. Ele roda como unidade `systemd --user` e
cuida de:

- captura pelo portal `InputCapture` + `libei` (papel de servidor);
- clipboard;
- geometria das telas, para `ir-geometry`;
- notificações ao usuário.

Ele **não** injeta. Se ele morrer, a máquina continua funcionando como cliente,
inclusive na tela de bloqueio — o que é o comportamento desejado.

## 6. Clipboard

| Necessidade | Mecanismo |
|---|---|
| Ler e escrever fora de foco | `wlr-data-control` / `ext-data-control`, quando o compositor expõe |
| Alternativa | portal `org.freedesktop.portal.Clipboard`, dentro de sessão `RemoteDesktop` |
| Arquivos | portal `FileTransfer` (`org.freedesktop.portal.FileTransfer`) |

Formatos canônicos do protocolo: texto UTF-8 com LF, imagem PNG, arquivos por manifesto.
O GNOME não expõe `wlr-data-control`; ali o caminho é o portal. A matriz do que está
disponível é medida em tempo de execução e mostrada no diagnóstico — nunca presumida.

Quando não há sessão de usuário (greeter), não há clipboard. A interface diz isso.

## 7. Unidade `systemd`

```ini
[Unit]
Description=InputRemote daemon
After=bluetooth.target network.target
Wants=bluetooth.target

[Service]
Type=notify
ExecStart=/usr/libexec/inputremote-daemon
Restart=on-failure
RestartSec=2

User=inputremote
Group=inputremote
CapabilityBoundingSet=
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictNamespaces=yes
RestrictRealtime=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
SystemCallFilter=@system-service
SystemCallArchitectures=native
StateDirectory=inputremote
ConfigurationDirectory=inputremote
RuntimeDirectory=inputremote
DeviceAllow=/dev/uinput rw

[Install]
WantedBy=multi-user.target
```

`ProtectHome=read-only`, e não `yes`: enviar ao par o que o usuário copiou da pasta pessoal exige
que o serviço leia ali ([ADR-0011](adr/0011-clipboard-na-travessia.md)). *O que* ele pode ler é
decidido pelo `ir-files`, que só envia o que o próprio usuário que pediu leria; o sandbox garante
que gravar em `/home` continua impossível. Um serviço com usuário dedicado, como o deste modelo,
também não leria uma pasta pessoal `0700` — enquanto o serviço for esse, o envio a partir dela
precisará que o ajudante do usuário entregue o conteúdo, e não o caminho.

`Type=notify`: o serviço só se declara pronto depois que os dispositivos `uinput` foram
confirmados pelo `udev` (§2.2). Assim, "serviço ativo" significa "capaz de injetar".

## 8. Bluetooth

BlueZ pelo D-Bus do sistema, com `org.bluez.ProfileManager1.RegisterProfile` publicando o
UUID do InputRemote e devolvendo um descritor de arquivo por conexão. O pacote instala uma
política em `/etc/dbus-1/system.d/` concedendo ao usuário `inputremote` acesso a
`org.bluez` — a política padrão do BlueZ só contempla `root` e o console.

O pareamento em nível de sistema operacional é **manual**, feito pelo usuário nas
configurações de Bluetooth do ambiente — o produto não registra `Agent1` nem dirige o
pareamento. Ver [ADR-0005](adr/0005-bluetooth-rfcomm-winsock.md); é a decisão que remove
a maior área não validada do v1.

Pareamentos do BlueZ ficam em `/var/lib/bluetooth/` e são da máquina, não do usuário —
por isso o enlace sobe antes do login, que é o que o requisito exige.

## 9. Armadilhas conhecidas, com resposta

| Armadilha | Resposta |
|---|---|
| Eventos perdidos logo após `UI_DEV_CREATE` | criar na subida e confirmar pelo `libudev` antes de declarar pronto |
| Aceleração aplicada duas vezes no ponteiro | injeção absoluta pelo dispositivo `EV_ABS` |
| Diálogo de permissão do portal a cada reconexão | persistir e reapresentar o `restore_token` |
| Compositor sem `InputCapture` | sem papel de servidor; dizer qual compositor e o que falta |
| Tecla presa após queda do enlace | `StateSnapshot` reconciliado; `ReleaseAll` em toda falha |
| Auto-recaptura: o `uinput` do cliente sendo lido pela captura do próprio cliente | o serviço ignora eventos dos dispositivos que ele mesmo criou, por nome e por `udev` |
| `libei` indisponível ou versão antiga | detecção em tempo de execução com mensagem específica, nunca falha genérica |
| GNOME sem `wlr-data-control` | usar o portal de clipboard |
| Suspensão deixando o enlace morto | `sleep.target` com `ReleaseAll` antes de dormir |
| Serviço sem acesso a `/dev/uinput` em sistema com SELinux | política SELinux no pacote RPM; testar no Fedora com *enforcing* |

A linha da auto-recaptura não é teórica: um cliente que também é servidor lê seus próprios
dispositivos virtuais e cria um laço de realimentação. O filtro por dispositivo de origem
é obrigatório desde o primeiro commit.
