# 12 — Painel no Android

**Status:** implementado, falta a bancada com aparelho de verdade · **Data:** 2026-10-24 ·
**Decisões:** [ADR-0018](adr/0018-painel-no-android.md) · ([log 68](logs/68-o-painel-no-android.md))

Um tablet ou celular Android, pareado com o sistema, mostra numa tela fixa o uso de CPU e RAM, as
temperaturas, o disco, a rede, a bateria e a latência do Bluetooth e da rede **dos computadores
conectados**. Um APK simples, que só observa, e que custa quase nada às máquinas.

## 1. O que é, e o que não é

| É | Não é |
|---|---|
| um painel de leitura, sempre aceso, para deixar ao lado do monitor | um controle remoto: o Android **nunca** manda comando |
| números do momento e o último minuto em mini-gráfico | um histórico de horas nem um banco de métricas |
| um par a mais do sistema, com o mesmo pareamento | um serviço web, um app na nuvem ou uma conta |
| um custo que só existe enquanto alguém está olhando | um coletor sempre ligado em segundo plano |

## 2. Decisões

Da entrevista com o usuário, e as três que mudaram na implementação (marcadas), pelos motivos do
[ADR-0018](adr/0018-painel-no-android.md):

| Tema | Decisão | Por quê |
|---|---|---|
| Portador | **rede local (Wi-Fi)** | não disputa o rádio Bluetooth, que leva teclado e mouse |
| Topologia | **um PC agrega**: o Android pareia com um computador, e ele repassa os dados do par | um pareamento só e uma conexão só no Android; o PC já tem a sessão com o par |
| Segurança | **o mesmo modelo do sistema**: Noise + código de seis dígitos, com o painel num vocabulário que só lê | não se abre exceção à regra de que tudo é cifrado e autenticado |
| Métricas | CPU, RAM, temperaturas, latência por rota, disco, tráfego de rede, bateria | tudo na v1 |
| Frequência | **1 s**, e **zero** sem ninguém olhando | o custo some quando a tela do Android apaga |
| Temperatura no Windows | zona térmica do ACPI e, **se estiver rodando**, o LibreHardwareMonitor | sem driver de núcleo nosso ([11](11-nao-legado.md)) |
| APK | Kotlin + Compose, **núcleo em Rust por JNI** *(mudou: era UniFFI)* | nove funções e eventos em JSON; sem passo de geração e sem o JNA no aparelho |
| Pareamento | **a descoberta de sempre acha o computador**, e o IP digitado é o plano B *(mudou: era QR)* | sem câmera nem biblioteca de leitura; quem autentica é o código, não o QR |
| Latência por rota | o `Pong` medido por portador antes da detecção de repetição *(detalhado)* | sem sonda nova; ver §3.2 |
| Android mínimo | **8.0 (API 26)** | cobre os tablets velhos, que são os que viram painel |
| Tela | sempre ligada, fundo preto com deslocamento contra marca, paisagem e retrato, protetor de tela ao carregar | é um painel, não um app que se abre e fecha |
| Histórico | **mini-gráfico de 60 s** por bloco, só na memória do Android | a latência oscila, e um número solto engana |
| Alertas | **cor por limite**, com o nível escrito ao lado, sem notificação | o painel já está à vista |
| Escopo | **só leitura** | a menor superfície de ataque possível |

## 3. Arquitetura

```
 ┌───────────── Android ─────────────┐        ┌────── PC que agrega ───────┐       ┌── PC par ──┐
 │ Compose: só desenha               │        │ inputremote-daemon         │       │ daemon     │
 │   ↑ StateFlow<Tela>               │ Wi-Fi  │  ir-vigilancia             │ sessão│  coleta só │
 │ Controlador (estados, religar)    │◄──────►│   ├ ir-monitor-rede (52527)│◄─────►│  se pedida │
 │   ↑ FonteDoPainel                 │ Noise  │   ├ ir-telemetria          │ v11   │            │
 │ libir_monitor_android.so (JNI)    │        │   └ Mesa (ir-monitor)      │       │            │
 └───────────────────────────────────┘        └────────────────────────────┘       └────────────┘
```

### 3.1. O fluxo, e por que o custo é zero sem ninguém olhando

1. A tela do Android aparece; ele liga e manda `Assinar(true)`, renovado a cada 2 s.
2. O computador liga a **sua** coleta e pede ao par a amostra dele (`EntrePcs::Assinar`, dentro de
   `Control::Panel`) pela sessão que já existe, também renovado a cada 2 s.
3. A cada segundo cada lado coleta; o computador junta as amostras e as voltas de cada rota num
   `Quadro`, codificado uma vez e cifrado para cada painel.
4. A tela do Android some: a ligação fecha. Sem renovação, as duas assinaturas **expiram em 5 s** —
   inclusive a de um tablet que caiu da rede sem avisar — e as duas coletas param.

Sem assinatura não há linha de coleta, nem arquivo de `/proc` aberto, nem consulta ao WMI.

### 3.2. Protocolo

- **Versão 11** entre os computadores: `Control::Panel`, um recado opaco para a sessão
  ([03, §6](03-protocolo.md)). Dentro dele, `ir_monitor::EntrePcs`: o pedido (com os processos que
  o painel quer), parar, e a amostra com os processos. Com um par mais antigo, o painel mostra só o
  computador que agrega e diz "Atualize o InputRemote neste computador" no cartão do outro.
- **`UsageSample` em ponto fixo**: CPU em milésimos, memória e disco em KiB, tráfego em bytes/s,
  bateria em %, até 8 temperaturas em centésimos de grau. `None` é "não sei ler". Uma amostra
  completa tem cerca de 40 bytes.
- **A latência não tem sonda nova.** O `Pong` que já existe é medido **por portador, antes da
  detecção de repetição** (`Notice::RouteLatency`). Na rota dupla o par responde ao primeiro `Ping`
  que lhe chega, então a medida da rota mais lenta é a ida pela mais rápida somada à volta por ela —
  uma estimativa honesta de quanto aquela rota atrasa, exata com um portador só. O painel mostra a
  mediana dos últimos 10 s de cada rota, e chama de "mais rápida" a de mediana menor.
- **O enlace do painel** (`ir-monitor`) é outro: TCP 52527, Noise `XX` com o código para parear,
  `IK` com a chave fixada para voltar, e um vocabulário (versão 2, vetores gravados) que só tem
  `Ola`, `Assinar` e `Interesse` vindo do painel. Os `Detalhes` — a cópia em curso e os processos —
  só vão a quem disse interesse, e o aplicativo só o diz a um computador da versão 2.

### 3.3. Coleta: `ir-telemetria`

```rust
/// Uma fonte lê uma coisa só e escreve o campo dela na amostra.
pub trait Fonte {
    fn ler(&mut self, agora: Instant, amostra: &mut UsageSample);
}
```

- Cada fonte tem uma responsabilidade; o `Coletor` depende do *trait*, e não do sistema. Uma fonte
  nova (uma GPU por NVML, por exemplo) entra sem mexer nele.
- **Cada fonte na sua cadência**: processador, memória e rede a 1 s; temperatura e bateria a 5 s;
  disco a 30 s. Entre leituras, o último valor.
- Os limites de alerta e os textos da tela (`ir_monitor::nivel`, `ir_monitor::vista`) existem **uma
  vez**, no Rust; o aplicativo recebe o bloco pronto e só escolhe a cor.

| Métrica | Windows | Linux |
|---|---|---|
| CPU | `GetSystemTimes` | `/proc/stat` |
| RAM | `GlobalMemoryStatusEx` | `/proc/meminfo` (total menos disponível) |
| Temperatura | `MSAcpi_ThermalZoneTemperature`; o WMI do LibreHardwareMonitor ou do OpenHardwareMonitor, se rodando | `hwmon` de drivers conhecidos |
| Disco | `GetDiskFreeSpaceExW` na unidade do sistema | `statvfs("/")` |
| Rede | `GetIfTable2`, só placa física e sem filtro | `/proc/net/dev`, sem as interfaces virtuais |
| Bateria | `GetSystemPowerStatus` | `/sys/class/power_supply` |
| RTT por rota | sessão (`ir-session`) | sessão (`ir-session`) |

Os arquivos de `/proc` e `/sys` ficam abertos durante a assinatura; a conexão WMI é aberta uma vez.
Cada tipo de sensor mostra o mais quente.

**Processos** ([log 70](logs/70-processos-e-copia-no-painel.md)): só com interesse — um card de
processo ou a lista aberta, aqui ou no painel do par. Uma foto por segundo, e o uso é a diferença
entre duas, agrupado pelo nome; um card soma tudo que contém o nome (`wsl` pega o `VmmemWSL`).

| | Windows | Linux |
|---|---|---|
| A foto | `NtQuerySystemInformation(SystemProcessInformation)`: uma chamada, 6–9 ms | `/proc/<pid>/stat` de cada processo |
| Memória | *working set* privado (a coluna do Gerenciador de Tarefas) | `rss` |

### 3.4. Orçamento de recursos (critério de aceite)

| Recurso | Teto no PC | Como está |
|---|---|---|
| CPU do serviço, a mais | **< 0,1 %** de um núcleo | **a medir** na bancada (§7) |
| Memória, a mais | **< 2 MB** | **a medir** |
| Rede | **< 1 KB/s** por painel | um quadro com dois computadores tem poucas centenas de bytes |
| Sem assinatura | **zero** | provado nos testes: sem assinante a vigia não existe |

No Android: APK de release com **4,4 MB**; a biblioteca nativa tem ~1,1 MB em arm64.

## 4. Pareamento e segurança

- **No PC**: tela inicial → "Painel no celular" → "Parear aparelho". A janela de pareamento fica
  aberta por três minutos, e a porta 52527 só abre com ela ou com um painel já pareado.
- **No aparelho**: "Parear" lista os computadores que a descoberta achou (UDP 52524); o IP digitado
  serve para a rede que isola os clientes.
- **Os dois mostram os seis dígitos, e os dois confirmam** ([04, §3.2](04-seguranca.md)). Recusar em
  qualquer lado avisa o outro na hora.
- **A chave do painel fica em `paineis`**, longe de `peers`: nenhum caminho que dá entrada,
  clipboard ou arquivo a um par consegue achá-la. O vocabulário que o computador aceita do painel
  só tem "olá" e "assinar".
- **Voltar** é `IK` com a chave do computador fixada. Se ele não está no último IP, o aparelho o
  reencontra pela descoberta: o id da instalação deriva da chave, e o DHCP só muda onde ele está.
- **Esquecer** pela janela do PC derruba a conexão do painel na hora; ele não volta sem parear.
- Parear um painel exige `Configurar`, e não elevação: ele não decide quem digita.

## 5. O aplicativo Android

### 5.1. Camadas

```
ui/telas/   Compose: Painel, TelaDoPainel, TelasDePareamento, Componentes (bloco, mini-gráfico, dígitos)
ui/         Controlador (a máquina de estados) → StateFlow<Tela>; PainelViewModel; Preferencias
dados/      FonteDoPainel e Ligacao (interfaces), modelos, Historico
nucleo/     FonteNativa e Leitura (JSON) sobre Nativo (JNI)
```

- **O Kotlin não tem regra de produto**: os textos, os níveis e os limites chegam prontos.
- **A tela depende de `FonteDoPainel`**, não do nativo: o `Controlador` é testado com uma fonte de
  mentira.
- **A ligação segue a tela**: abre em `onStart`, fecha em `onStop`. Girar o aparelho não a derruba.
- **Religar** com espera de 1, 2, 4… até 30 s, sempre pela chave gravada.
- **Dado velho**: sem quadro há mais de 3 s, o conteúdo esmaece e o topo diz "sem dados há 12 s".

### 5.2. Layout

Pensado **deitado** — é como um tablet fica ao lado do monitor —, e travado deitado por padrão
("Sempre deitado" no menu ⋮; desligado, gira com o aparelho). No tablet de 8" (853 × 533 dp
deitado) tudo cabe sem rolar ([log 69](logs/69-o-painel-deitado-e-os-cards.md)):

- **um computador por coluna**; em cada uma, **o processador e a memória primeiro, grandes e lado a
  lado** — número de 36 sp, barra grossa e o último minuto em gráfico;
- **o resto em linhas compactas**: valor e detalhe lado a lado, barra fina, sem gráfico. O valor
  nunca é cortado; quem encolhe, com reticências, é o detalhe;
- **a faixa "Entre os computadores"** numa linha fina embaixo e, enquanto há cópia de arquivos,
  **a faixa da cópia** ao lado dela: o que vai, quanto, a velocidade e quanto falta. Em pé, as faixas
  vão para o topo e os computadores se empilham.

**O "i"** no processador e na memória abre a lista de processos daquele computador, ao vivo:
ordenada pelo processador ou pela memória, conforme o "i" tocado; as linhas só trocam de lugar a cada
5 s, para não andarem debaixo do dedo; e cada uma tem **+ Acompanhar**, que vira um card. Os cards de
processo (até quatro) são o que o computador mede: tirados, são esquecidos.

**Os cards são da pessoa** (`dados/Arranjo.kt`): no menu ⋮, **Editar painel** põe em cada card
◀ ▶ (mudar a ordem), ★ (destaque ou compacto) e ✕ (tirar), e no topo **Adicionar card** (só
oferece o que existe nesta máquina), **Restaurar padrão** e **Concluir**. Vale para os dois
computadores, e fica guardado no aparelho. O arranjo só conhece a `chave` de cada métrica, então uma
métrica nova de uma versão futura entra no fim sozinha, e uma que a pessoa tirou continua tirada.

- **Cores por nível**, com o nível **escrito** ("atenção", "crítico") ao lado da cor; a borda do
  cartão na cor do pior nível dele; "—" quando a máquina não sabe ler.
- Limites da v1 (`ir_monitor::nivel`): CPU 75/90 %, memória e disco 80/90 %, temperatura 75/85 °C,
  latência 20/50 ms, bateria fora da tomada 20/10 %.

### 5.3. Modo painel

- **Tela sempre ligada** enquanto o aplicativo está à vista; **escurecer** no menu ⋮.
- **Fundo preto e deslocamento** de alguns pontos a cada minuto, contra marca no OLED.
- **Protetor de tela** (`PainelDream`): o Android mostra o painel "ao carregar" ou "na base", sem
  permissão especial — desde o Android 8 um aplicativo não recebe o aviso de carregador pelo
  manifesto, e desde o 10 não abre Activity em segundo plano. A pessoa o escolhe uma vez em
  *Configurações → Tela → Protetor de tela*.

### 5.4. Primeiro uso

1. No PC: "Painel no celular" → "Parear aparelho".
2. No aparelho: abrir o aplicativo; o computador aparece na lista; tocar em "Parear".
3. As duas telas mostram o mesmo código; "São iguais" nas duas.
4. O painel aparece. Das próximas vezes, abre direto nele.

## 6. Fora da v1

- Ações no Android (o "Resolver" da economia do Wi-Fi, "Reconectar Bluetooth").
- Limites configuráveis, alerta com som ou vibração, histórico de horas.
- iOS, widget de tela inicial, Android TV.
- GPU por NVML/ADLX sem o LibreHardwareMonitor: entra como mais uma `Fonte`.
- Mostrar o par quando o computador que agrega está desligado.

## 7. Plano e estado

| Etapa | Entrega | Estado |
|---|---|---|
| E0 | as fontes nos dois sistemas | feito: `cargo run -p ir-telemetria --example amostra` no Windows e no Fedora |
| E1 | `ir-telemetria` | feito, 23 testes |
| E2 | protocolo 11 e latência por rota | feito: vetores gravados, testes de sessão com a rota dupla |
| E3 | porta 52527, pareamento, papel de só leitura | feito: `ir-monitor`, `ir-monitor-rede`, `ir-vigilancia`; testes em TCP de verdade |
| E3b | a janela do PC | feito, conferida em `--simulado` |
| E4 | o aplicativo | feito: emulador de tablet contra a bancada (`cargo run -p ir-monitor-rede --example bancada`) |
| E5 | modo painel | feito no código; **falta** uma noite inteira num aparelho no carregador |
| E6 | o orçamento da §3.4 medido, com o serviço instalado e um aparelho de verdade | **falta** |
| E7 | a cópia em tempo real, os cards de processo e a lista do "i" | feito: tablet de verdade contra a bancada ([log 70](logs/70-processos-e-copia-no-painel.md)) |

## 8. Riscos

| Risco | Mitigação |
|---|---|
| a zona térmica do ACPI volta vazia em muitos PCs | "—" com "sem sensor legível"; o LibreHardwareMonitor, se rodando |
| o computador que agrega dorme | o painel diz que está conectando e tenta sozinho; não mostra número velho como atual |
| a economia de energia do Wi-Fi do aparelho atrasa os quadros | o painel só esmaece depois de 3 s sem quadro |
| um segundo artefato para assinar e distribuir | o Gradle compila o núcleo sozinho; release com R8 |

## 9. Em aberto

- **Distribuição do APK**: *releases* do GitHub, F-Droid, ou os dois — e a chave de assinatura.
- **Mais de um aparelho ao mesmo tempo**: o monitor já conta os assinantes e só para com o último;
  falta a bancada com dois.
- **O computador que agrega desligado**: parear o aparelho com os dois e usar o que estiver de pé.
