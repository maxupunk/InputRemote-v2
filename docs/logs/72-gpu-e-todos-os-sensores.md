# A placa de vídeo e todos os sensores de temperatura

**Data:** 2026-10-26 · **Branch:** `feat/painel-android`

**Itens:** [12, §3.3 e §5.2](../12-painel-android.md); [PROGRESSO](../../PROGRESSO.md), Etapa 12.

**O que foi pedido:** "card de temperatura e uso de GPU, e card de todos os sensores de temperatura
conforme o computador, com alta eficiência para não usar muito recurso".

## O que havia

Uma temperatura por parte da máquina — a mais quente do processador, da placa de vídeo, dos discos e
da placa-mãe —, sem nome; no Linux, só os `hwmon` de drivers conhecidos; do processador, no Windows,
só o pacote. Da placa de vídeo, nada além da temperatura.

## O que mudou

### A amostra (`ir-proto`)

- **Cada temperatura tem nome** ("Package", "Core #3", "GPU Hot Spot", "Disco 2", "Zona TZ00"), até
  `MAX_SENSOR_NAME` = 16 bytes, e há até `MAX_TEMPERATURES` = 12 (eram 8). `Sensor::Other` no fim
  do enum, para o que a máquina mede e o painel não classifica (a placa de rede, a bateria).
- **`gpus: Vec<Gpu>`**, até duas — a integrada e a dedicada de um notebook: o fabricante, o uso em
  milésimos (o motor mais ocupado) e a memória dedicada (`None` numa integrada).
- **A amostra mudou de crate**: do `ir-proto` para o `ir-monitor` (`amostra.rs`). Desde que a
  versão 11 virou o recado opaco ([log 70](70-processos-e-copia-no-painel.md)), nenhuma mensagem do
  protocolo a usa — ela é vocabulário do painel. O `ir-proto` tinha passado do teto de 2 500 linhas
  com os campos novos; a fronteira já estava provada, e a mudança só a escreveu.
- **Exceção de pré-lançamento**: a versão 11 e o vocabulário 2 do painel nunca saíram da branch; o
  vetor gravado do quadro e o do recado `uso` (`ir-monitor/tests/vetores.rs`) foram regravados no
  lugar. Doze sensores com nome são uns 240 bytes, e a amostra cabe no recado do rádio
  (`MAX_PANEL_MESSAGE` = 440) com a lista de processos encolhendo, como já encolhia.

### O que se lê, e quanto custa

- **`sensores.rs`** (puro): de que parte é cada sensor e como ele se chama, nos dois sistemas. Com
  mais de doze, a escolha é **justa** — uma vaga para o mais quente de cada parte, depois o segundo
  de cada uma —: um processador de dezesseis núcleos não esconde a placa de vídeo e os discos. Dois
  com o mesmo nome viram "NVMe" e "NVMe 2", porque o nome é a chave do card.
- **Linux**: todo `hwmon`, com o rótulo do driver (`tempN_label`), lido uma vez ao começar a
  assinatura; os arquivos de temperatura continuam abertos e só relidos. A placa de vídeo pelo
  `/sys/class/drm` (`gpu_busy_percent`, `mem_info_vram_*` do `amdgpu`); a Intel e a NVIDIA não
  escrevem uso ali, e o card delas diz "sem leitura de uso" — perguntar ao `nvidia-smi` a cada
  segundo seria um processo novo por segundo.
- **Windows, temperaturas**: os núcleos do processador entram (antes, só o pacote), sem "Distance to
  TjMax" (é quanto falta, e não temperatura) nem a média; a zona do ACPI ganha o nome dela.
- **Windows, placa de vídeo**: o DXGI dá o fabricante, a memória dedicada e o LUID, uma vez por
  assinatura; `D3DKMTQueryStatistics` dá, por placa, o tempo de cada motor e os bytes de cada
  segmento. O uso é o motor mais ocupado entre duas leituras (`gpu.rs`, puro) — a conta do
  Gerenciador de Tarefas. **Cerca de 1 ms por leitura** nesta máquina.

### A tela

- O card **GPU** entra sozinho (no padrão, logo depois da memória): o uso, a memória ("1,0 GB de
  6,0 GB") ou o fabricante; com duas placas, "GPU · NVIDIA" e "GPU · Intel". A cor segue a memória
  de vídeo, e não o uso: a placa a 100 % num jogo é o normal dela.
- Os cards de cada parte (`temp-cpu`…) continuam com as mesmas chaves — o painel arrumado de quem já
  usa segue igual —, e dizem qual sensor é o mais quente ("mais quente: Core #3").
- **Um card por sensor** (`sensor:cpu:core-3`), oferecido em "Adicionar card" numa seção própria,
  "Sensores de temperatura", com rolagem; não entra sozinho, e tirado é esquecido — a regra "sob
  demanda" dos cards de processo, agora com nome (`Arranjo.ehSobDemanda`).

## O que foi provado

- Testes: a escolha justa, os nomes de cada fonte, o uso pelos motores, os cards por parte e por
  sensor, os de uma e de duas placas, o arranjo sob demanda e o que "Adicionar card" oferece; os
  vetores regravados; `clippy` e `cargo xtask check`.
- Nesta máquina, pelo `--example amostra`: a NVIDIA (6 GB, parada, Optimus) e a Intel (de 2 % a
  6,5 %), conferidas contra o `Get-Counter` do sistema. As temperaturas vêm vazias como usuário
  comum: a zona do ACPI só abre para o serviço, como `SYSTEM`.

- **Linux, sem contêiner**: o Docker Desktop respondia erro 500, então o código do Linux foi
  verificado por `cargo clippy --target x86_64-unknown-linux-gnu` a partir do Windows, com o `clang`
  do NDK compilando o C do `ring` (`CC_x86_64_unknown_linux_gnu`, `--sysroot` do NDK; o `clippy` não
  linka). `ir-telemetria`, `ir-monitor`, `ir-vigilancia` e `ir-transferencia`, com `--all-targets` e
  `-D warnings`: sem aviso. A lógica que o Linux usa (`sensores`, `gpu`, `numero_do_sys`) é pura e
  roda nos testes de qualquer sistema; ler os arquivos de uma máquina Linux de verdade falta.

- **No fedora de verdade** (Fedora 44, kernel 7.2, notebook com `coretemp`, `nvme`, `acpitz` e
  GPU Intel): o `--example amostra`, compilado no contêiner Fedora 44 e rodado lá, leu o Package e os
  núcleos mais quentes, o NVMe, cinco zonas do ACPI e a placa Intel ("sem leitura de uso", como
  esperado). No contêiner, `clippy` do workspace sem aviso e os testes dos crates tocados.

## O que deu errado no caminho

- **A primeira leitura de GPU custava caro.** Pelos contadores de desempenho
  (`\GPU Engine(*)\Utilization Percentage`), que enumeram um item por processo em cada motor, a
  batida do coletor passou a levar de 0,5 a 0,9 s. Trocada pelas estatísticas do núcleo de vídeo.
- **O relógio de parede mentia**: com o rust-analyzer compilando e o processador a 100 %, até
  `GetSystemTimes` "levava" meio segundo. A medida que decide é o tempo de CPU da linha
  (`GetThreadTimes`): todas as fontes juntas ficaram abaixo do quantum de 15,6 ms em cinco de sete
  batidas.
- O hexadecimal da placa vem em maiúsculas do PDH e em minúsculas do `Get-Counter`; as chaves passaram
  a ser comparadas em minúsculas (antes de o PDH sair).
- Um teste do log 70 exigia tempo de processador maior que zero de um processo recém-criado — que
  pode ainda não ter tempo nenhum. Tirado.

- **Duas coisas que só a máquina real mostrou**: um sensor do NVMe respondendo −273,15 °C (o zero
  absoluto, o jeito de o driver dizer que não mede) — agora uma leitura fora de −40 °C a 150 °C
  fica de fora; e quatro zonas do ACPI sem rótulo viravam "acpitz", "acpitz 2", "acpitz 2",
  "acpitz 2" — a numeração contava pelos nomes já renumerados, e três cards teriam a mesma chave.
  Agora conta pelo nome original, e a zona sem rótulo se chama "Zona ACPI".

## O que falta

- Com o serviço **instalado** (como `SYSTEM`), ver no tablet os sensores do ACPI, e os do
  LibreHardwareMonitor quando ele estiver aberto.
- Uma placa AMD no Linux, para o uso e a memória pelo `/sys`.
