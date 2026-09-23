# 07 — Stack e dependências

## 1. A escolha da linguagem

A pergunta não é "qual linguagem é melhor", é "qual linguagem atende **estes** quatro
eixos ao mesmo tempo". Os eixos saem direto dos requisitos de [01](01-visao-e-escopo.md):

| Eixo | Origem |
|---|---|
| **A. Sem runtime e sem coletor de lixo** | o binário precisa injetar entrada na tela de login, cedo no boot, com latência p99 previsível |
| **B. Acesso direto a API nativa** | `CreateProcessAsUser`, `SendInput`, `SendSAS`, `AF_BTH`, `ioctl` de `uinput`, `EVIOCGRAB`, D-Bus |
| **C. Segurança de memória no decodificador** | um processo `SYSTEM` que analisa bytes vindos de um rádio aberto, antes de qualquer login |
| **D. Um código só para Windows e Linux** | manter duas implementações do produto é o caminho para o v1 acontecer de novo |

| Linguagem | A | B | C | D | Veredito |
|---|:-:|:-:|:-:|:-:|---|
| **Rust** | ✔ | ✔ | ✔ | ✔ | escolhida |
| C++ moderno | ✔ | ✔ | ✘ | ~ | perde só no eixo C — e o eixo C é indispensável aqui |
| C# / .NET | ✘ | ✔ (Win) | ✔ | ✘ | runtime na tela de login e GC no caminho do ponteiro |
| Go | ✘ | ~ | ✔ | ~ | GC, e API nativa só por `cgo`/`syscall` verboso |
| Zig | ✔ | ✔ | ~ | ✔ | linguagem ainda instável; ecossistema Bluetooth e D-Bus inexistente |

### 1.1. Por que o eixo C decide

C++ é a alternativa séria, e em acesso a API nativa é ligeiramente melhor. Mas o
componente mais exposto deste produto é o decodificador de protocolo: ele roda como
`SYSTEM`, processa bytes de um rádio Bluetooth que qualquer um pode alcançar, e faz isso
**antes de haver qualquer usuário logado na máquina**. Um estouro de buffer ali não é um
travamento — é execução remota de código com privilégio máximo, sem autenticação.

Em Rust, esse decodificador é código seguro por construção, verificável com
`#![forbid(unsafe_code)]` no crate inteiro. Em C++, seria uma promessa sustentada por
revisão e disciplina. Para este componente específico, é a diferença entre uma garantia e
uma intenção.

### 1.2. O que se perde escolhendo Rust, dito honestamente

- APIs Win32 e `ioctl` exigem `unsafe` de qualquer forma — Rust não elimina o risco nos
  *backends*, apenas o confina a módulos pequenos e marcados;
- tempo de compilação é maior que o de Go e comparável ao de C++;
- `libei`, portais e BlueZ têm bindings de Rust menos maduros que os de C;
- o v1 já era Rust e falhou — o que prova que a linguagem não protege contra arquitetura
  ruim, e é por isso que [09](09-padroes-de-codigo.md) existe.

Ver [ADR-0002](adr/0002-rust.md).

## 2. Dependências, com justificativa

Uma dependência só entra se a resposta a "por que não dá para viver sem ela" couber numa
linha. Versões são fixadas na implementação, depois da PoC correspondente.

### Núcleo

| Área | Escolha | Por que não dá para viver sem |
|---|---|---|
| Runtime assíncrono | `tokio` | três portadores, dois IPCs e temporizadores concorrentes; só nos crates de E/S |
| Serialização | `postcard` + `serde` | binário compacto, decodificação sem alocação, Rust puro |
| Hash | `blake3` | verificação de arquivo e impressão digital de chave, rápido e moderno |
| Criptografia | `snow` | implementação madura do Noise; uma camada só para os três portadores |
| Apagar segredos | `zeroize`, `subtle` | material de chave em memória e comparação em tempo constante |
| Erros | `thiserror` (libs), `anyhow` (binários) | fronteira explícita entre erro de biblioteca e de aplicação |
| Logs | `tracing` + `tracing-appender` | escrita sem bloqueio, exigida por [02, §6](02-arquitetura.md) |
| Configuração | `toml` + `serde` | editável à mão quando a interface não abre |
| Imagem do clipboard | `png` | o protocolo leva imagem em PNG e o Windows a guarda em DIB; codificar e decodificar PNG à mão seria zlib e CRC nossos no caminho de dados de outro programa. Rust puro, sem `unsafe` ([log 45](logs/45-a-varredura-implementada.md)) |

**Não usamos PAKE.** O v1 dependia de `spake2 0.5.0-pre` — uma versão pré-lançamento no
caminho de segurança. Ela é desnecessária: como o produto já exige comparação visual do
código nos dois lados, um Noise_XX seguido de código curto derivado do hash do handshake
(*short authentication string*) dá a mesma resistência a homem no meio, com uma
dependência a menos e nada para o usuário digitar. Ver [04, §3.2](04-seguranca.md).

### Rede

| Área | Escolha | Observação |
|---|---|---|
| UDP e TCP | `tokio::net` | sockets crus; a confiabilidade do canal de entrada é nossa, e é pequena ([03, §4.1](03-protocolo.md)) |
| Descoberta | própria, por broadcast (`ir_net::descoberta`) + `if-addrs` | o `mdns-sd` saiu: no Windows o serviço (SYSTEM) não compartilha a 5353 com programas do usuário ([03, §10](03-protocolo.md)) |
| *Keepalive* do TCP | `socket2` | o canal de dados precisa saber que o par sumiu sem esperar horas pelo *keepalive* do sistema; o `tokio` não expõe os tempos ([log 45](logs/45-a-varredura-implementada.md)) |

**Não usamos QUIC.** `quinn` + `rustls` + `rcgen` + `tokio-rustls` somavam quatro
dependências grandes no v1 para resolver, na rede, um problema que o Bluetooth continuava
resolvendo de outro jeito. Ver [ADR-0003](adr/0003-noise-em-vez-de-quic.md).

### Plataforma

| Área | Windows | Linux |
|---|---|---|
| API do sistema | `windows` (crate oficial) | `rustix` |
| Serviço | `windows-service` | `sd-notify` |
| Bluetooth | Winsock `AF_BTH` pelo crate `windows` | `bluer` (BlueZ por D-Bus) |
| Entrada | `SendInput`, `WH_*_LL`, Raw Input | `evdev` + `uinput` por `ioctl`; `reis` para `libei` |
| Portais | — | `ashpd` |
| Clipboard | `clipboard-win` | protocolos Wayland próprios + `ashpd` |

**Não usamos WinRT.** Ver [ADR-0005](adr/0005-bluetooth-rfcomm-winsock.md) e
[00, §3](00-licoes-do-v1.md).

### Interface

| Área | Escolha |
|---|---|
| Janela | `slint` |
| Bandeja | `tray-icon` |

## 3. Slint e a licença do projeto

O produto é MIT. O Slint não é, e isso precisa estar decidido antes da primeira linha,
não depois.

O Slint é oferecido sob GPLv3, sob uma licença livre de *royalties* para aplicações
desktop, ou sob licença paga para embarcados. Para um projeto de código aberto de desktop,
os dois primeiros servem, sem custo.

Decisão: **o crate `ir-ui` usa a licença livre de royalties do Slint e exibe a atribuição
exigida** (um item "Sobre" com o aviso do Slint). O restante do repositório permanece MIT.

Isso funciona porque `ir-ui` é um binário separado que depende apenas de `ir-ipc`
([02, §2](02-arquitetura.md)). Nenhum crate do produto liga contra o Slint. Se um dia a
licença do Slint mudar de forma inaceitável, troca-se um binário — não o produto.

Essa contenção é um efeito colateral do desenho de três processos, e é uma das razões
concretas para ele.

## 4. Ferramentas de desenvolvimento

| Ferramenta | Uso | Onde |
|---|---|---|
| `cargo fmt` | formatação | CI, bloqueante |
| `cargo clippy` | `-D warnings`, com `pedantic` | CI, bloqueante |
| `cargo test` | unidade e integração | CI, bloqueante |
| `cargo deny` | licenças, avisos RustSec, duplicatas | CI, bloqueante |
| `cargo fuzz` | decodificador do `ir-proto` | CI noturno, e antes de cada lançamento |
| `cargo llvm-cov` | cobertura de `ir-session` e `ir-proto` | CI, com piso de 85% nesses dois |
| `cargo about` | avisos de licença dos artefatos | lançamento |
| `cargo xtask` | verificação das regras de [09](09-padroes-de-codigo.md) | CI, bloqueante |

## 5. Compilação e empacotamento

Preservado do v1, que acertou nisto: **um comando gera os dois pacotes**, com o pacote
Linux construído a partir do Windows por contêiner.

```text
scripts/build.ps1              → dist/
    InputRemote-<v>-windows-x86_64-setup.exe    instalador assinado, registra o serviço
    InputRemote-<v>-windows-x86_64.zip          portátil, com install.bat/uninstall.bat
    inputremote-<v>.fc<n>.x86_64.rpm            unidade systemd, regra udev, política SELinux
    inputremote_<v>_amd64.deb                   idem
```

Cada artefato leva `.sha256` e assinatura. No Windows, **todos os binários** precisam ser
assinados, por duas razões independentes: distribuir um serviço `SYSTEM` sem assinatura é
irresponsável, e o UIAccess do agente — sem o qual não se digita na tela de bloqueio — só
é concedido a binário assinado, instalado em local gravável apenas por administradores
([05, §4.4](05-windows.md)).

Isso torna o certificado de assinatura de código uma **dependência do projeto**, no mesmo
nível de uma biblioteca. O pacote portátil continua existindo e serve para uso normal, mas
carrega a limitação declarada.

Alvos mínimos, a confirmar na PoC-0:

| | Mínimo |
|---|---|
| Windows | 10 22H2 x86-64 |
| Fedora | 42 |
| Debian / Ubuntu | Debian 13, Ubuntu 24.04 LTS |
| Rust | edição 2024, MSRV fixada e testada no CI |
| Portais | `xdg-desktop-portal` 1.21+ para o papel de servidor |
| BlueZ | 5.66+ |

## 6. O que deliberadamente não entra

| Descartado | Motivo |
|---|---|
| `quinn`, `rustls`, `rcgen`, `tokio-rustls` | uma camada de cripto só, sobre os três portadores |
| `spake2` | desnecessário com comparação visual; e a versão publicada é pré-lançamento |
| WinRT (`Windows.Devices.*`) | não é alvo suportado para serviço na sessão 0 |
| `eframe` / `egui` | interface imediata acoplada ao produto no v1 |
| `gtk` | dependência pesada para uma tela de configurações |
| Tauri / WebView | `webkit2gtk` como requisito de instalação no Linux, para uma janela de configuração |
| Atualizador automático | superfície de ataque num serviço `SYSTEM` ([01, §4](01-visao-e-escopo.md)) |
| Telemetria | não há coleta; nem opcional |
