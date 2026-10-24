# 12 — Painel no Android

**Status:** proposta · **Data:** 2026-10-24 · **Origem:** entrevista com o usuário

Um tablet ou celular Android, pareado com o sistema, mostra numa tela fixa o uso de CPU e RAM, as
temperaturas, o disco, a rede, a bateria e a latência do Bluetooth e da rede **dos computadores
conectados**. Um APK simples, que só observa, e que custa quase nada às máquinas.

Este documento registra a ideia e os rumos. As decisões que mudam a arquitetura (um papel novo de
par e uma porta nova na rede) viram um ADR quando a implementação começar.

## 1. O que é, e o que não é

| É | Não é |
|---|---|
| um painel de leitura, sempre aceso, para deixar ao lado do monitor | um controle remoto: o Android **nunca** manda comando |
| números do momento e os últimos 60 s | um histórico de horas nem um banco de métricas |
| um par a mais do sistema, com o mesmo pareamento | um serviço web, um app na nuvem ou uma conta |
| um custo que só existe enquanto alguém está olhando | um coletor sempre ligado em segundo plano |

## 2. Decisões da entrevista

| Tema | Decisão | Por quê |
|---|---|---|
| Portador | **rede local (Wi-Fi)** | não disputa o rádio Bluetooth, que leva teclado e mouse; alcança qualquer PC da casa |
| Topologia | **um PC agrega**: o Android pareia com um computador, e ele repassa os dados do par | um pareamento só e uma conexão só no Android; o PC já tem a sessão com o par |
| Segurança | **o mesmo modelo do sistema**: Noise + código de 6 dígitos ([04, §3](04-seguranca.md)), com papel *monitor* | não se abre exceção à regra de que tudo é cifrado e autenticado |
| Métricas | CPU, RAM, temperaturas, latência por rota, disco, tráfego de rede, bateria | escolha do usuário; tudo na v1 |
| Frequência | **1 s**, e **zero** sem ninguém olhando | o custo some quando a tela do Android apaga |
| Temperatura no Windows | fontes oficiais, e o **LibreHardwareMonitor só se já estiver rodando** | sem driver de kernel nosso ([11](11-nao-legado.md)); quem quer mais sensores já tem o LHM |
| APK | **Kotlin + Jetpack Compose**, com o **núcleo em Rust** via UniFFI | o Noise, os quadros e o SAS não são reescritos (DRY): é o mesmo código do desktop |
| Android mínimo | **8.0 (API 26)** | cobre os tablets velhos, que são os que viram painel |
| Tela | sempre ligada, tema escuro com proteção contra marca (*burn-in*), paisagem e retrato, abrir sozinho ao carregar | é um painel, não um app que se abre e fecha |
| Histórico | **mini-gráfico de 60 s** por card, só na memória do Android | a latência oscila, e um número solto engana; custo zero no PC |
| Alertas | **cor por limite**, sem notificação, com limites fixos na v1 | o painel já está à vista, e notificação é ruído |
| Escopo | **só leitura** | a menor superfície de ataque possível |

## 3. Arquitetura

```
 ┌──────────── Android ────────────┐        ┌──────── PC agregador ────────┐       ┌──── PC par ────┐
 │ Compose (UI)                    │        │ inputremote-daemon           │       │ daemon         │
 │   ↑ StateFlow                   │  Wi-Fi │  ├─ escuta do monitor (TCP)  │ sessão│  ├─ coletor    │
 │ PainelViewModel                 │◄──────►│  ├─ coletor local            │◄─────►│  └─ (só se     │
 │   ↑                             │ Noise  │  └─ repasse do par           │  já   │     alguém     │
 │ Repositório ── núcleo Rust (FFI)│        │                              │ existe│     olha)      │
 └─────────────────────────────────┘        └──────────────────────────────┘       └────────────────┘
```

### 3.1. O fluxo, e por que o custo é zero sem ninguém olhando

1. O Android abre a conexão e manda `Assinar`.
2. O agregador liga o **seu** coletor e manda ao par `Telemetria{assinar: true}` pela sessão que
   já existe (canal 0, controle).
3. A cada segundo, cada lado coleta, monta uma `Amostra` e o agregador a entrega ao Android. A do par
   chega pela sessão e é repassada sem ser decodificada de novo.
4. O Android apaga a tela, sai do primeiro plano ou some da rede. O agregador manda
   `Telemetria{assinar: false}`, e os dois coletores param.

A assinatura **expira sozinha** em 5 s sem renovação. Um Android que caiu sem avisar não deixa
coletor ligado para sempre. Assim, sem assinatura não há timer, nem leitura de `/proc`, nem
consulta WMI: o painel não existe para o PC.

### 3.2. Protocolo

- **Versão 11**. `Telemetria{assinar}` e `Amostra` só vão para um par que fale a 11, como as
  versões anteriores já fazem com as mensagens novas ([03, §6](03-protocolo.md)). Com um par mais
  antigo, o painel mostra só o agregador e avisa "o outro computador precisa ser atualizado".
- **`Amostra` é compacta e em ponto fixo**: CPU em permilagem (`u16`), RAM e disco em KiB (`u64`),
  temperaturas como lista `(sensor, centésimos de °C em i16)`, tráfego em bytes/s, bateria em %
  com o estado, e o RTT de cada rota em microssegundos (`Option<u32>`, sem valor = rota caída).
  Fica abaixo de **300 bytes por amostra**, ou seja, menos de 1 KB/s com dois PCs.
- **A latência não é medida de novo.** O `Ping`/`Pong` com `stamp_micros` já roda na sessão, e a
  sessão já guarda o último RTT (`last_rtt`, em `ir-session`). Falta só separar **por rota**
  (Bluetooth e rede), ver §9.
- **A conexão do monitor é outro listener**, em TCP na rede local, com o mesmo Noise. Ela só fala
  `Assinar` e `Amostra`. Qualquer outra mensagem fecha a conexão.

### 3.3. Coleta: crate `ir-telemetria`

Um crate novo, com o núcleo sem E/S ([ADR-0004](adr/0004-nucleo-sans-io.md)) e as fontes atrás
de uma interface por métrica:

```rust
/// Uma fonte lê uma coisa só. Quem não consegue ler devolve `None`, e o painel mostra "—".
pub trait Fonte {
    fn ler(&mut self, agora: Instante) -> Option<Leitura>;
}
```

- **SOLID.** Cada fonte tem uma responsabilidade (CPU, memória, térmica, disco, rede, bateria). O
  coletor depende do *trait*, e não do sistema operacional. Uma fonte nova, como a GPU por NVML,
  entra sem mexer no coletor (aberto e fechado).
- **DRY.** A montagem da `Amostra`, os limites e as cores são definidos **uma vez**, no Rust, e
  o Android os recebe pela FFI. O Kotlin não recalcula limite.
- **Cada fonte tem a sua cadência.** CPU, RAM, rede e RTT a cada 1 s. Temperatura e bateria a cada
  5 s. Disco a cada 30 s. O coletor repete o último valor entre uma leitura e outra. É isso que
  segura o custo do WMI.

| Métrica | Windows | Linux |
|---|---|---|
| CPU | `GetSystemTimes` (diferença entre duas leituras) | `/proc/stat` |
| RAM | `GlobalMemoryStatusEx` | `/proc/meminfo` |
| Temperatura | zona térmica ACPI (`MSAcpi_ThermalZoneTemperature`, o serviço já roda como SYSTEM). Se existir o namespace WMI `root\LibreHardwareMonitor`, os sensores dele: CPU, GPU e disco | `/sys/class/hwmon`, `/sys/class/thermal` |
| Disco | `GetDiskFreeSpaceExW` | `statvfs` |
| Rede | `GetIfTable2` (diferença de bytes) | `/proc/net/dev` |
| Bateria | `GetSystemPowerStatus` | `/sys/class/power_supply` |
| RTT por rota | sessão (`ir-session`) | sessão (`ir-session`) |

Os arquivos de `/proc` e `/sys` ficam **abertos** e são relidos com `pread` desde o início, sem
reabrir a cada segundo. A conexão WMI é aberta uma vez por assinatura, e não a cada leitura.

### 3.4. Orçamento de recursos (critério de aceite)

Medido em bancada, nos dois sistemas, com um tablet assistindo:

| Recurso | Teto no PC |
|---|---|
| CPU do daemon, a mais | **< 0,1 %** de um núcleo (média de 5 min) |
| Memória, a mais | **< 2 MB** |
| Rede | **< 1 KB/s** por tablet |
| Sem assinatura | **zero**: nenhuma thread acordando, nenhum timer |

E no Android: CPU abaixo de 3 % com a tela acesa, sem *wake lock* fora da tela visível, e um APK de
cerca de 5 MB.

## 4. Pareamento e segurança

- **O pareamento é o mesmo**, e o Android vira um par com o papel `monitor`. Na interface do PC:
  "Parear painel" mostra um **QR** com o endereço, a porta e a chave pública do PC. O Android lê o
  QR, faz o aperto de mão Noise, e **os dois mostram os 6 dígitos**, comparados a olho, como entre
  dois PCs ([04, §3.2](04-seguranca.md)). O QR só evita digitar o endereço. Quem autentica é o SAS.
- **O papel `monitor` fica gravado na identidade do par**, e o transporte recusa entrada, clipboard,
  arquivos e pasta para ele antes de decodificar qualquer coisa. É uma lista de permissões
  fechada, e não uma lista de proibições.
- **O PC só escuta a porta do monitor** se houver um painel pareado ou se a janela de pareamento
  estiver aberta (os mesmos 3 minutos de [04, §3](04-seguranca.md)).
- **O que um painel vazado revela:** carga, temperatura e latência, nada de conteúdo. Ainda
  assim vai cifrado, porque padrão de uso diz se há alguém em casa.
- **Revogar** fica na lista de pares da interface do PC, como qualquer outro par.

## 5. O app Android

### 5.1. Camadas

```
ui/        Compose: PainelScreen, CardDoPc, Bloco, Sparkline, PareamentoScreen
apresentação/   PainelViewModel → StateFlow<EstadoDoPainel>   (sem lógica de protocolo)
dados/     RepositorioDoPainel → MonitorCliente (núcleo Rust via UniFFI)
domínio/   modelos imutáveis: Computador, Amostra, Nivel (ok/atenção/crítico)
```

- **Uma Activity só**, com MVVM e *unidirectional data flow*. O ViewModel não conhece rede, e o
  repositório não conhece tela.
- **A conexão segue o ciclo de vida**: abre em `onStart` e fecha em `onStop`. Sem serviço em
  primeiro plano, sem trabalho em segundo plano. Se a tela apagou, o PC para de coletar (§3.1).
- **Reconexão** com espera crescente (1, 2, 4, até 30 s) e um aviso discreto "reconectando…".
- O crate `ir-monitor-ffi` expõe ao Kotlin só `parear`, `conectar`, `assinar` e um fluxo de
  `Amostra`. É uma superfície pequena, gerada pelo UniFFI e compilada com `cargo-ndk` para
  `arm64-v8a` e `armeabi-v7a`.

### 5.2. Layout

Tablet deitado, um PC por coluna:

```
┌───────────────────────────────────┬───────────────────────────────────┐
│ ● DESKTOP-MSI            Windows  │ ● notebook               Fedora   │
│                                   │                                   │
│  CPU  23 %  ▁▂▂▃▅▃▂▁▂▃            │  CPU   8 %  ▁▁▂▁▁▁▂▁▁▁            │
│  RAM  11,2 / 32 GB   ▓▓▓▓░░░░░░   │  RAM  5,1 / 16 GB    ▓▓▓░░░░░░░   │
│  CPU 61 °C   GPU 48 °C   SSD 39°C │  CPU 54 °C   SSD —                │
│  Disco 412 GB livres              │  Disco 88 GB livres   🔋 76 % ⚡   │
│  Rede ↓ 1,2 MB/s  ↑ 80 KB/s       │  Rede ↓ 30 KB/s  ↑ 4 KB/s         │
├───────────────────────────────────┴───────────────────────────────────┤
│  Rota   Bluetooth 7 ms ▁▁▂▁▁   ·   Rede 3 ms ▁▁▁▁▁   ·  vence: rede   │
└───────────────────────────────────────────────────────────────────────┘
```

No celular em pé, os cards ficam um embaixo do outro e a faixa da rota vai para o topo.

- **A rota é uma faixa só**, embaixo dos dois PCs, porque a latência é do **enlace** entre eles, e
  não de uma máquina.
- **Cores por nível**, com limites fixos na v1: CPU ≥ 90 %, RAM ≥ 90 %, temperatura ≥ 85 °C,
  latência ≥ 50 ms e rota caída ficam em âmbar ou vermelho. Ao lado da cor vai sempre um ícone ou
  texto, para não depender só da cor (acessibilidade).
- **Dado velho fica cinza**: sem amostra há mais de 3 s, o card mostra "sem dados há 12 s", e não
  o último número como se fosse atual.
- **"—" quando a fonte não existe**, por exemplo um sensor que o Windows não expõe sem o LHM. Um
  toque longo explica o motivo.
- Números com dígitos de largura fixa (`tabular-nums`), para o texto não tremer a cada segundo.

### 5.3. Modo painel

- **Tela sempre ligada** (`FLAG_KEEP_SCREEN_ON`) só com o app visível, com brilho reduzido
  opcional.
- **Fundo preto e proteção contra marca:** o conteúdo se desloca alguns dp a cada minuto. Em OLED,
  preto é pixel apagado.
- **Abrir sozinho ao carregar: pelo protetor de tela do Android (`DreamService`).** O próprio
  sistema mostra o painel "ao carregar" ou "na base", e isso não exige permissão especial. É o
  caminho certo porque, desde o Android 8, um app não pode ouvir `ACTION_POWER_CONNECTED` pelo
  manifesto, e, desde o 10, não pode abrir Activity em segundo plano. O usuário escolhe
  "InputRemote" em *Configurações → Tela → Protetor de tela* uma vez, e o app explica o passo na
  primeira vez.

### 5.4. Primeiro uso

1. Abrir o app, tocar em **Parear**, e a câmera abre.
2. No PC: **Parear painel**, e o QR aparece.
3. Ler o QR. Os dois mostram `482 913`, e o usuário confirma no PC.
4. O painel já aparece, com os dois computadores.

São três toques, sem nada para digitar.

## 6. Fora da v1

- Ações no Android (o "Resolver" da economia do Wi-Fi, "Reconectar Bluetooth"): o escopo é só
  leitura.
- Limites configuráveis, alerta com som ou vibração, histórico de horas.
- iOS, widget de tela inicial, Android TV.
- GPU por NVML/ADLX sem o LHM: entra depois como mais uma `Fonte` (§3.3).

## 7. Plano

| Etapa | Entrega | Prova |
|---|---|---|
| E0 | spike das fontes nos dois sistemas, medindo o custo de cada uma, o WMI em especial | tabela de custo no log |
| E1 | `ir-telemetria`: *trait*, fontes e montagem da `Amostra` | testes unitários com leituras gravadas, verdes nos dois sistemas |
| E2 | protocolo 11: `Telemetria`/`Amostra` entre os PCs, assinatura que expira | teste de sessão e bancada com os dois PCs |
| E3 | escuta do monitor, papel `monitor` e pareamento por QR + SAS | teste de recusa: o monitor manda entrada e a conexão cai |
| E4 | APK mínimo: pareamento e painel com os dois PCs | bancada com o tablet |
| E5 | modo painel: `DreamService`, proteção contra marca, retrato e paisagem | uma noite inteira ligado ao carregador |
| E6 | medição do orçamento (§3.4) e documentação em `USAR.md` | log com os números |

## 8. Riscos

| Risco | Mitigação |
|---|---|
| a consulta WMI à zona térmica é cara ou volta vazia em muitos PCs | cadência de 5 s, conexão aberta uma vez, e "—" sem drama; o E0 mede |
| o agregador dorme e o painel fica vazio | o painel diz "DESKTOP-MSI dormindo", e não "erro"; ver §9 |
| a economia de energia do Wi-Fi do tablet atrasa as amostras | 1 s de tolerância já cobre; o card só fica cinza após 3 s |
| um toolchain Android novo no repositório (Gradle, NDK) | isolado em `android/`, fora do `cargo build` do resto; o CI ganha um job próprio |

## 9. Em aberto

- **RTT por rota.** A sessão guarda o último RTT (`last_rtt`), mas a rota dupla precisa dele
  **separado** para o Bluetooth e para a rede, além de saber qual rota está vencendo. É preciso
  conferir em `ir-session` se o `Pong` já identifica o portador.
- **Se o agregador cair:** o Android pareia também com o outro PC e usa o que estiver de pé? Isso
  dobra o pareamento, mas tira o ponto único. A sugestão é deixar para depois da v1.
- **Mais de um tablet ao mesmo tempo:** a assinatura conta os assinantes e só para com o último.
  É barato, mas precisa entrar no E2.
- **Distribuição do APK:** *sideload* pelas *releases* do GitHub, F-Droid ou as duas?
- **Descoberta:** o QR já leva o endereço. Se o IP do PC mudar (DHCP), o Android redescobre por
  mDNS (`_inputremote-painel._tcp`) ou pede para ler o QR de novo?
