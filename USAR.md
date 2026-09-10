# Como usar agora (Windows → Linux, sessão desbloqueada)

Este é o caminho testável hoje: o teclado e o mouse do **Windows** controlam o **Linux**, com as
duas telas desbloqueadas. A conexão é por **rede (UDP)**, cifrada com Noise, e o pareamento pede a
comparação de um código de 6 dígitos nas duas telas.

> A tela de bloqueio (nível N2/N3) ainda não entra aqui — ela depende do agente no desktop seguro
> e da assinatura, que são as próximas etapas. Isto cobre o nível **N1**: as duas máquinas
> desbloqueadas.

## O que é preciso

- As duas máquinas na **mesma rede local**, e o IP de cada uma.
- No **Linux**, o serviço precisa de acesso a `/dev/uinput`. Para o teste, o jeito mais simples é
  rodar com `sudo` (a instalação como serviço, com regra `udev` e usuário dedicado, é a Etapa 1.5).

## 1. Compilar

No Windows:

```powershell
cargo build --release -p ir-daemon
```

No Linux (ou pelo container do Fedora, como o resto do projeto):

```bash
cargo build --release -p ir-daemon
```

O executável fica em `target/release/inputremote-daemon`.

## 2. Configurar

O serviço guarda estado em `IR_DATA_DIR` (padrão `./ir-state`). Na primeira execução ele cria um
`config.toml` que você edita, e gera a identidade da máquina.

**No Linux (cliente, que é controlado):** `ir-state/config.toml`

```toml
role = "client"
peer_edge = "left"     # o Windows fica à esquerda do Linux
port = 52526
screen_width = 1920    # a resolução da tela do Linux
screen_height = 1080
peers = []
```

**No Windows (servidor, que tem o teclado):** `ir-state\config.toml`

```toml
role = "server"
peer_edge = "right"          # o Linux fica à direita do Windows
port = 52525
screen_width = 1920          # detectado sozinho no Windows; o valor aqui é reserva
screen_height = 1080
peer_addr = "192.168.0.24:52526"   # IP:porta do LINUX
peers = []
```

Troque `192.168.0.24` pelo IP real do Linux.

## 3. Parear (só na primeira vez)

1. **No Linux**, suba o serviço primeiro (ele fica escutando):

   ```bash
   sudo IR_DATA_DIR=./ir-state RUST_LOG=info ./target/release/inputremote-daemon
   ```

2. **No Windows**, suba o serviço (ele conecta):

   ```powershell
   $env:IR_DATA_DIR="./ir-state"; $env:RUST_LOG="info"
   .\target\release\inputremote-daemon.exe
   ```

3. Os **dois** terminais mostram o mesmo código, por exemplo `=== CÓDIGO DE PAREAMENTO: 086395 ===`.
   Confira que são iguais nas duas telas. Se forem, digite `s` e Enter **nos dois**. Se forem
   diferentes, digite `n` — códigos diferentes significam que alguém pode estar no meio da conexão.

4. Os dois logam `sessão estabelecida ... carrier=udp`. Pronto: a chave do par fica gravada, e das
   próximas vezes a conexão é automática, sem código.

## 4. Usar

Encoste o ponteiro na **borda direita** da tela do Windows. Ele atravessa para o Linux, e o
teclado e o mouse passam a controlar o Linux. Para voltar, encoste na borda esquerda da tela do
Linux.

Se algo travar, o atalho de emergência solta tudo e devolve o controle (a ser ligado na interface;
por ora, encerrar o serviço no Windows devolve o controle e solta todas as teclas).

## O que ainda não está aqui

- **Tela de bloqueio (N2/N3):** precisa do agente no desktop seguro e da assinatura de código.
- **Bluetooth:** o portador principal do projeto; hoje só a rede UDP está implementada.
- **Interface gráfica ligada ao serviço:** a janela já existe (`inputremote-ui`), mas ainda fala
  com um serviço simulado; ligá-la ao serviço de verdade é o próximo passo.
- **Clipboard e arquivos.**

O caminho completo até a tela de bloqueio está em [docs/08-plano-de-implementacao.md](docs/08-plano-de-implementacao.md).
