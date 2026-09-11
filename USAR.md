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

Quem passa teclado e mouse é o **agente**, que o serviço lança sozinho dentro da sessão do
usuário — um serviço na sessão 0 não alcança a área de trabalho de ninguém. Você não precisa
iniciá-lo: o serviço o sobe e, se ele cair, o ressobe em poucos segundos.

> **O que ainda não foi exercitado:** o lançamento entre sessões e a passagem de entrada com o
> serviço instalado ainda não foram rodados numa máquina de verdade — são justamente os dois
> itens do primeiro teste físico. Se algo não passar, o caminho em **primeiro plano** descrito
> acima é o de referência, e o diagnóstico em Preferências diz se o agente está de pé.

## Instalar no Linux (RPM)

O pacote traz a **interface e o serviço**. O serviço não sobe sozinho depois de instalado:

```bash
sudo dnf install ./dist/inputremote-0.1.0-*.rpm
sudo usermod -aG inputremote "$USER"   # sem isto a janela não fala com o serviço
sudo systemctl enable --now inputremote
sudo systemctl status inputremote      # confira que está "active (running)"
journalctl -u inputremote -f           # é aqui que aparecem as linhas da tabela abaixo
```

**Reinicie o computador** depois do `usermod`. Sair e entrar na sessão **não basta** no GNOME: o
gerenciador da sessão (`systemd --user`) sobrevive ao logout enquanto houver qualquer outra sessão
sua aberta — um terminal por SSH, por exemplo —, e os programas da nova sessão gráfica nascem dele,
com os grupos de antes. A janela continua sem acesso, e a faixa amarela continua lá.

Se não quiser reiniciar agora, abra a janela já com o grupo, por um terminal:

```bash
sg inputremote -c inputremote-ui
```

Cuidado ao conferir com `id` num terminal: um terminal novo pode mostrar o grupo mesmo quando a
sessão gráfica ainda não o tem. O que vale é o grupo do processo da janela.

A configuração do serviço fica em `/var/lib/inputremote/config.toml` — edite `role`,
`peer_edge`, `port` e `screen_width`/`screen_height` como na seção 2, e reinicie com
`sudo systemctl restart inputremote`.

> No Linux **não há agente**: a injeção por `uinput` entra abaixo do compositor, e quem a faz é o
> próprio serviço. O agente existe só no Windows, onde um serviço na sessão 0 não alcança a área
> de trabalho do usuário.
>
> **Por que o grupo.** O serviço roda como root (é quem tem `/dev/uinput`), então o socket de
> controle nasceria `root:root` e a janela — que roda sem privilégio — levaria "permissão
> negada" e cairia para o simulado. Em vez de abrir o socket para todo mundo, ele fica
> `0660 root:inputremote`: quem opera a máquina entra nesse grupo de propósito, e o acesso vira
> uma decisão registrada do administrador.
>
> Se preferir não mexer em grupos, dá para **parear pelo terminal**: o `journalctl` mostra o
> código de seis dígitos, e o serviço aceita `s` pela entrada padrão quando rodado à mão com
> `sudo`.

## Como saber que está funcionando

Suba o serviço com `RUST_LOG=info` e procure estas linhas, nesta ordem. Cada uma diz que uma
peça entrou no lugar, e a **primeira que faltar** é onde está o problema:

| Linha no registro | O que ela confirma |
|---|---|
| `canal de controle no ar` | a janela tem por onde falar com o serviço |
| `canal do agente no ar` | o agente tem por onde conectar |
| `agente lançado pid=…` | o serviço conseguiu criar o processo do agente |
| `agente conectado` | o agente achou o serviço |
| `agente pronto desktops=[…]` | o agente está capturando e pronto para injetar |
| `tela da sessão do usuário largura=… altura=…` | a resolução veio de dentro da sessão (confira se é a sua) |
| `interface conectada` | a janela achou o serviço — **se não aparecer, ela está no simulado** |
| `código de pareamento: NNNNNN` | compare com a outra tela antes de confirmar |
| `par gravado` | o par foi fixado; das próximas vezes não pede código |
| `sessão estabelecida … carrier=udp` | as duas máquinas estão de pé e falando |

Depois disso, encoste o ponteiro na borda configurada. Se o agente cair, o serviço registra
`o agente saiu` e o ressobe em poucos segundos.

## O que ainda não está aqui

- **Tela de bloqueio (N2/N3):** precisa da thread por desktop no agente e da assinatura de código.
- **Bluetooth:** o portador principal do projeto; hoje só a rede UDP está implementada.
- **Descoberta automática (mDNS) na janela:** por ora, **Procurar** mostra o par configurado em
  `peer_addr`; a descoberta na rede é um refinamento posterior.
- **Clipboard e arquivos.**

O caminho completo até a tela de bloqueio está em [docs/08-plano-de-implementacao.md](docs/08-plano-de-implementacao.md).
