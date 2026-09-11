# 14 — O serviço de ponta a ponta: IPC, pareamento pela janela e o serviço que sobe

**Data:** 2026-09-10

## O problema

A instalação pelo MSI falhava com "Service 'InputRemote' failed to start", mesmo com
administrador. E, ainda que subisse, não havia como parear sem terminal: a janela
(`inputremote-ui`) falava com um serviço simulado, e o código de seis dígitos só aparecia no
`stdout` do daemon em primeiro plano.

Duas lacunas, uma raiz comum: **faltava o transporte entre os três processos** do
[desenho de arquitetura](../02-arquitetura.md) — serviço (dono do estado), interface (pareia pela
tela) e agente (captura e injeta na sessão do usuário) — e faltava o daemon saber conversar com o
Gerenciador de Serviços do Windows (SCM).

## O que entrou

### O canal de controle (interface ↔ serviço)

Um transporte de IPC local, com um *named pipe* no Windows e um socket Unix no Linux, enquadrado
pelo mesmo `ir_ipc::codec` do resto do projeto (prefixo de 4 bytes + `postcard`, com o limite de
tamanho conferido **antes** de alocar — o serviço roda privilegiado e não pode ser levado a alocar
por um número vindo de fora).

- **No serviço** (`ir-daemon/src/ipc/`): `tokio`, uma tarefa por conexão. Lê `Pedido`, encaminha
  ao ator — o único dono do estado — e devolve a `Resposta`; em paralelo, empurra os `Aviso`
  (o código de pareamento, a conclusão, a mudança de estado) para quem pediu para acompanhar.
- **Na interface** (`ir-ui/src/real.rs`): biblioteca padrão, bloqueante, porque o laço do Slint
  não é `tokio`. Uma thread de leitura separa respostas de avisos; `pedir` serializa escrita e
  resposta sob um cadeado só. Se o serviço não está no ar, a janela **cai para o simulado** e
  avisa — uma interface que finge estar ligada é pior que uma que diz que não está.

O `ParaInterface` (envelope que distingue `Resposta` de `Aviso` no mesmo fluxo) foi o tipo que
faltava em `ir-ipc` para isso.

### O ator traduz, e não vaza

A tradução entre o estado interno (`ir_session::Phase`, `Carrier`, `Role`) e o `ir_ipc::Estado`
publicado mora no ator (`actor/pedidos.rs`), do lado de dentro da fronteira. A interface continua
sem conhecer o produto: ela pede e mostra. `IniciarPareamento` vira `Connect{Pair}`,
`ConfirmarPareamento` vira a mesma confirmação que o terminal dava, e a comparação dos seis
dígitos **não é pulável** — nem pela tela, nem por configuração.

### O serviço que sobe (SCM)

`ir-daemon/src/service.rs` registra o daemon no SCM com o `windows-service`: responde a iniciar,
parar e interrogar, e reporta "rodando" a tempo. Sem isso, o `StartService` do instalador estoura
o tempo — que era exatamente a falha da imagem. Fora do SCM (execução à mão, para teste), o mesmo
binário cai para primeiro plano, então nada do fluxo de teste muda.

O estado do serviço, sem `IR_DATA_DIR`, fica em `%ProgramData%\InputRemote`, que o SYSTEM sabe
escrever.

### O serviço não cai por não capturar

Rodando como serviço na sessão 0, a captura e a injeção diretas não alcançam a sessão do usuário
— é para isso que existe o agente. Enquanto ele não entra, `build_io` **não derruba o serviço**
se um backend de entrada falhar: o serviço fica de pé, o canal de controle funciona, o pareamento
pela tela funciona, e só a passagem de teclado e mouse é que espera o agente.

## Como foi verificado

- **Transporte:** um teste (`ipc::controle::tests`) sobe o servidor num *named pipe* real,
  conecta um cliente, e confirma que um pedido recebe resposta e que um aviso empurrado chega a
  quem acompanha. Passa.
- **Sem regressão no pareamento:** dois daemons em *loopback*, cada um com o canal de controle no
  ar, ainda pareiam e estabelecem a sessão (`par gravado` + `sessão estabelecida carrier=udp`).
- **Primeiro plano intacto:** o daemon detecta que não foi lançado pelo SCM (código 1063) e roda
  em primeiro plano como antes.
- `cargo xtask`, `clippy --all-targets` e `fmt --check` limpos; a suíte inteira passa.

O que **não** deu para verificar nesta máquina: subir de verdade como serviço SYSTEM (precisa de
elevação) e a passagem de entrada entre sessões (precisa do agente e de uma segunda máquina).

## O que isto destrava

O caminho usável hoje, sem instalar e sem administrador: **daemon em primeiro plano nas duas
máquinas + pareamento pela janela**. A janela encontra o serviço, mostra o código, e a confirmação
nas duas telas fecha o par. Depois de pareado, o mouse e o teclado cruzam.

## O que falta para o serviço instalado passar entrada

O **agente** na sessão do usuário (o segundo canal de IPC, e o serviço lançando o agente com
`CreateProcessAsUserW`). É o único pedaço entre "o serviço sobe e pareia" e "o serviço instalado
passa teclado e mouse na sessão do usuário".

**Decisões:** o transporte do servidor é `tokio` e o do cliente é bloqueante, porque o Slint não
é `tokio`; a sobrescrita `IR_CONTROL_ENDPOINT` aceita nome curto porque a barra invertida do
*pipe* não sobrevive a algumas camadas de shell; a falha de backend de entrada é aviso, não queda.
