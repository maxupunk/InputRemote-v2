# O agente que nunca chegava a dizer por quê

**Data:** 2026-09-16

**Itens:** tentativa de travessia de teclado e mouse por Bluetooth. Uma fronteira de segurança
confirmada e um defeito de cadência achado no caminho.

## O que eu queria, e o que consegui

O objetivo era ver teclado e mouse **atravessando** por rádio — o único item que faltava para
"tudo funcionando". O que consegui foi menos que isso, e mais do que eu esperava de brinde.

**De brinde:** as duas máquinas já estavam pareadas por rádio desde o
[log 27](27-o-par-que-voltava-sozinho.md), com a chave fixada e o `peer_addr` de cada uma
apontando para o endereço de rádio da outra. Ao subir a instância de teste, elas **reconectaram
sozinhas**, sem código e sem ninguém pedir:

```text
Windows  conectando ao par alvo=AC:50:DE:47:EB:28 portador=bluetooth
         enlace seguro pronto; iniciando a sessão
         portador escolhido carrier=bluetooth why=Preferred
Fedora   sessão estabelecida peer=SAMSUNG-MAXUEL carrier=bluetooth
```

Isso é a reconexão automática por Bluetooth, que era item aberto da Etapa 7.

**O que não consegui:** o controle nunca atravessou. O estado ficou `Pronto` antes e depois de
levar o cursor à borda esquerda, nunca `EmUso`.

## A fronteira que me barrou, e ela está certa

No Windows quem captura teclado e mouse é o **agente**, não o serviço — um serviço na sessão 0
não alcança a área de trabalho de ninguém. E o canal do agente é criado assim:

```rust
// Restrito de propósito: este canal carrega injeção de entrada, e nenhum processo do usuário
// pode abri-lo. O agente alcança por rodar como o próprio serviço.
let escuta = escuta::Escuta::abrir(&endereco_do_agente(), escuta::Acesso::Restrito)?;
```

`Acesso::Restrito` é o SDDL `D:P(A;;GA;;;SY)(A;;GA;;;BA)` — só SYSTEM e Administradores.

Todos os testes desta bancada rodaram numa **segunda instância sem elevação**, ao lado do serviço
instalado, usando `IR_DATA_DIR`, `IR_CONTROL_ENDPOINT` e `IR_AGENT_ENDPOINT`. Isso serviu para
rádio, pareamento, sessão e quadros. Para o agente, não serve — e não deve servir: minha
instância roda como `SAMSUNG-MAXUEL\Maxuel`, sem elevação, então cria o canal com aquele
descritor e o próprio agente que ela lança não consegue abri-lo.

O `lancar_agente` fecha o argumento: **como serviço** ele usa `CreateProcessAsUserW` com
duplicação de token para a sessão de console; **fora dele**, é `Command::new(exe).spawn()`, um
filho comum com o mesmo token sem elevação. Não há caminho de elevação no meu cenário, e não
deveria haver.

Ou seja: **a travessia de entrada no Windows exige o serviço instalado, rodando como SYSTEM.**
Não é defeito; é o [04, §5](../04-seguranca.md) funcionando. O que falta para esse teste é a
instalação do MSI, que depende de privilégio de administrador.

## O defeito que apareceu no caminho

Investigando *por que* o agente não ficava pronto, encontrei algo que não tem a ver com
privilégio, e que se esconde justamente quando mais faria falta.

| | Constante | Valor |
|---|---|---|
| O agente desiste depois de | `TENTATIVAS × ESPERA` (`ir-agent`) | 60 × 500 ms = **30 s** |
| O serviço relança o agente a cada | `RECONNECT_TICKS × 3` (`ir-daemon`) | 600 × 5 ms × 3 = **9 s** |

O serviço lançava um agente novo a cada 9 s enquanto o anterior ainda estava dentro dos seus 30 s
de tentativas. Duas consequências, e elas se escondiam uma na outra:

1. **O erro nunca chegava a aparecer.** O agente tem a mensagem certa pronta — `"não achei o
   serviço em {endereco}"` —, mas precisa de 30 s para chegar nela, e era morto antes. Quem
   depurasse recebia uma linha de "iniciando" e mais nada. Foi o que eu recebi.
2. **Os processos se empilhavam** — exatamente o que o comentário daquele trecho dizia estar
   evitando. A evidência estava no meu próprio registro, antes de eu entender: três
   `inputremote-agent` vivos ao mesmo tempo, iniciados às 06:54:44, 06:54:53 e 06:55:02. Nove
   segundos entre eles.

No caminho normal isso nunca aparece: com o serviço como SYSTEM, o agente conecta em
milissegundos. O defeito só se manifesta quando o agente **realmente** não consegue conectar — que
é precisamente a hora em que a mensagem de erro importa.

A correção é o espaçamento passar a ser maior que a janela de desistência: 36 s contra 30 s, com o
porquê escrito na constante. Os dois números vivem em crates diferentes e o compilador não tem
como amarrá-los, então a nota diz que mexer num obriga a conferir o outro.

## Uma correção ao que eu ia escrever

Cheguei a formular que "o agente falha em silêncio". **Está errado, e retiro.** Ele reporta com
contexto adequado; o que havia era uma janela de observação — a minha, de 5 s, e a do serviço, de
9 s — menor que o tempo que ele precisa para concluir que desistiu. Defeito de cadência, não de
diagnóstico ausente.

## O que ainda não foi provado

- **Teclado e mouse atravessando.** Precisa do MSI instalado, para o serviço rodar como SYSTEM.
- **Latência adicionada** medida como [01, §6](../01-visao-e-escopo.md) define, e a causa dos
  ~51 ms de ida e volta ([log 28](28-a-carga-que-refutou-o-sniff.md)).
- **O socket na sessão 0**, sem usuário logado.
- **Windows↔Windows**, e o par sobrevivendo a um reinício das duas máquinas.
