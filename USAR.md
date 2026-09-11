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
- As portas **UDP** de cada lado (52525 e 52526, ou as que você escolher) abertas no firewall. Na
  primeira execução, o Windows costuma perguntar se libera — responda que sim para redes privadas.
- **Uma tela em cada máquina** (o caso de vários monitores tem uma ressalva, no fim).

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

A **ordem de subida não importa**: quem estiver esperando tenta conectar a cada 3 segundos, e a
conexão se refaz sozinha depois de uma queda passageira.

### Se algo travar

Encerre o serviço no **Windows** (Ctrl+C na janela dele). O cliente percebe o silêncio em até 1
segundo e **solta todas as teclas e botões** automaticamente — é a rede de segurança contra tecla
presa. Se um dos lados ficar confuso (por exemplo, depois de reiniciar só um deles), encerre e suba
os dois de novo.

## Ressalvas conhecidas deste primeiro teste

- **Vários monitores:** o mapeamento assume uma tela por máquina. Com mais de um monitor, a borda
  de travessia e a posição do cursor podem ficar deslocadas — é um refinamento posterior.
- **Injeção no Linux (`uinput`):** o dispositivo virtual é criado seguindo o desenho de
  [docs/06-linux.md](docs/06-linux.md), mas só foi compilado, não exercitado numa máquina Linux com
  ambiente gráfico. Se o ponteiro não se mexer no Linux, é o primeiro ponto a investigar (confira
  que `/dev/uinput` está acessível e que o compositor reconheceu os dispositivos `InputRemote`).
- **Resolução do Linux:** ponha `screen_width`/`screen_height` na resolução real da tela do Linux,
  para o movimento e a borda de volta ficarem na proporção certa.

## Parear pela janela (em vez do terminal)

O passo 3 acima usa o terminal. Dá para fazer o mesmo pela **janela** (`inputremote-ui`), que é
o caminho normal:

1. Suba o daemon em primeiro plano nas duas máquinas (sem `IR_CONTROL_ENDPOINT`, para a janela
   achar o serviço no canal padrão).
2. Abra o `inputremote-ui` em cada uma. Se ela achar o serviço, a barra de "serviço simulado" some.
3. Numa delas, **Procurar** mostra o computador configurado em `peer_addr`; escolha-o e comece o
   pareamento. As duas janelas mostram os seis dígitos em caixas.
4. Confira que são iguais e confirme nas **duas** janelas. Deu certo, o par fica gravado.

A comparação dos seis dígitos não é pulável — nem pela tela, nem por configuração. É o que protege
contra alguém no meio da conexão.

## Instalar como serviço (Windows)

O MSI instala o daemon como serviço do Windows (sobe com a máquina, como SYSTEM). A instalação
agora **conclui** — o serviço registra no Gerenciador de Serviços e responde ao "iniciar" do
instalador. Depois de instalado, a janela pareia pelo serviço, igual ao primeiro plano.

> **Ressalva do serviço instalado:** rodando como SYSTEM na sessão 0, o serviço ainda **não passa
> teclado e mouse** para a sessão do usuário — isso depende do **agente**, que é a próxima etapa.
> Até lá, o caminho que passa entrada de verdade é o **primeiro plano** descrito acima. O serviço
> instalado sobe e pareia; a passagem de entrada aguarda o agente.

## O que ainda não está aqui

- **Agente na sessão do usuário:** o que falta para o serviço instalado passar teclado e mouse.
- **Tela de bloqueio (N2/N3):** precisa do agente no desktop seguro e da assinatura de código.
- **Bluetooth:** o portador principal do projeto; hoje só a rede UDP está implementada.
- **Descoberta automática (mDNS) na janela:** por ora, **Procurar** mostra o par configurado em
  `peer_addr`; a descoberta na rede é um refinamento posterior.
- **Clipboard e arquivos.**

O caminho completo até a tela de bloqueio está em [docs/08-plano-de-implementacao.md](docs/08-plano-de-implementacao.md).
