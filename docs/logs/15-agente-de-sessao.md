# 15 — O agente de sessão: o serviço alcança a sessão do usuário

**Data:** 2026-09-10

## O problema

Com o serviço já subindo e pareando pela janela ([log 14](14-servico-de-ponta-a-ponta.md)),
faltava a peça que faz o produto **funcionar** quando instalado: um serviço do Windows roda na
**sessão 0**, e de lá ele não enxerga o teclado e o mouse do usuário nem alcança o desktop dele.
Instalado, o serviço subia e pareava — e não passava uma tecla sequer.

Não é defeito nosso: é o isolamento de sessão do Windows, desenhado exatamente para impedir que
um serviço veja ou escreva na área de trabalho de alguém.

## A forma da solução

O agente. Um processo separado, lançado **pelo serviço dentro da sessão do usuário**, que captura
e injeta lá, e conversa com o serviço por um canal de IPC próprio.

### O agente não decide nada

Ele recebe "injete isto" e devolve "isto aconteceu"
([02, §1.2](../02-arquitetura.md)). Toda política continua no serviço, que é o único dono do
estado — e é isso que faz o agente ser **descartável**: ele pode morrer, e o serviço o ressobe
sem que nada se perca.

### Dois canais, e um deles não sabe dizer "injete"

O canal do agente é separado do canal da interface, com vocabulários que não se misturam. A razão
é de segurança, não de organização: `Injetar` **não existe** no vocabulário da interface. Se
qualquer processo do usuário pudesse pedir injeção ao serviço, qualquer programa que ele rodasse
poderia digitar no prompt de UAC ([04, §5](../04-seguranca.md)). A ausência é a garantia.

### Quem sabe o tamanho da tela é quem está na sessão

Duas coisas mudaram de lado por causa disso:

- O agente informa o arranjo de telas (`TelasMudaram`) ao conectar. Um serviço na sessão 0 lê
  métricas que não são as do usuário, e a travessia cairia na borda errada.
- O `PrenderPonteiro` viaja **normalizado**, e o agente o converte em pixels. A conversão ficou do
  lado que conhece a tela.

Também entrou `FatoDoAgente::PonteiroAbsoluto`: a posição real do cursor, e não o delta. É o que
faz a travessia disparar no ponto certo, em vez de acumular deslocamentos a partir de uma origem
arbitrária — o mesmo defeito que já tinha sido corrigido no caminho local.

### O lançamento

Dois caminhos, conforme quem o serviço é:

- **Em primeiro plano** (teste à mão), já estamos na sessão do usuário: um processo filho comum.
- **Como serviço** (`LocalSystem`, sessão 0): duplica o próprio token, move-o para a sessão de
  console, marca `TokenUIAccess` e cria o processo em `WinSta0\Default`. É o fluxo que a
  [PoC-1](07-poc1-tela-de-bloqueio.md) já tinha validado.

O serviço relança o agente enquanto não houver um de pé. Isso trouxe um defeito que só aparece
quando se pensa na corrida: o agente leva um instante para conectar depois de lançado, e insistir
antes disso poria **dois** agentes capturando a mesma sessão — cada tecla chegaria duplicada ao
par. A trava é dupla, e nos dois lados do problema:

- o canal aceita **um agente de cada vez**, e fecha o segundo na cara antes de ele instalar
  gancho nenhum (o agente só liga a captura depois de conectar);
- o relançamento acontece a cada três ciclos, e não a cada um, para não empilhar processos
  esperando a mesma vaga.

Só quem chegou a ocupar a vaga anuncia o encerramento — uma conexão recusada não pode derrubar o
agente bom. Agente que morre vira soluço, não fim de sessão.

### No Windows o serviço não toca mais em entrada

Deliberado: os backends locais de captura e injeção ficam **vazios** no Windows. Dois capturadores
(um no serviço, um no agente) competiriam e duplicariam cada evento. No Linux nada disso se
aplica — o serviço injeta direto por `uinput`, e nenhum agente conecta
([06, §2](../06-linux.md)). O despacho escolhe sozinho, sem `if` de plataforma espalhado: usa o
agente **quando há agente pronto**.

## Como foi verificado

- **O agente subiu de verdade.** Com o serviço em primeiro plano, o registro mostra o ciclo
  inteiro: `canal do agente no ar` → `agente lançado pid=…` → `agente conectado` →
  `tela da sessão do usuário largura=3440 altura=1440` → `agente pronto desktops=["Default"]`.
  A resolução é a da máquina de verdade, o que confirma que o arranjo de telas passou a vir de
  quem está na sessão. Ao derrubar o serviço, o ator registrou `o agente saiu` — a detecção de
  queda também funciona.
- **O canal do agente tem teste.** Um fato sobe ao ator, um comando desce ao agente, e um
  **segundo** agente que tente conectar é fechado na hora — a trava que impede dois capturadores
  na mesma sessão.
- **A pilha inteira, com o agente no caminho.** Dois serviços em *loopback*, cada um com o seu
  agente: os dois logaram `agente pronto`, `par gravado` e `sessão estabelecida carrier=udp`.
  É o que confirma que tirar a captura e a injeção de dentro do serviço no Windows não quebrou
  o pareamento nem a sessão.

Duas coisas apareceram na revisão e foram corrigidas junto: a interface lia
`IR_CONTROL_ENDPOINT` **sem** a expansão de nome curto que o serviço e o agente fazem — o mesmo
valor apontaria para lugares diferentes, e o sintoma seria a janela cair para o simulado sem
explicação. E o canal de controle passou a registrar `interface conectada`, que é como se
confirma no diagnóstico que a janela achou o serviço em vez de ter desistido em silêncio.
- Compila e passa limpo nos **dois** sistemas: Windows e Linux (este dentro do container do
  Fedora 44, que é o sistema de destino de verdade).
- `clippy --all-targets`, `cargo xtask` (tamanho, setas de dependência, privacidade dos logs) e
  `fmt --check` limpos nos dois; a suíte inteira passa.
- O MSI sai com os três componentes, sem ausentes (`daemon=1, agente=1`).
- O `actor` passou de 400 linhas com o que entrou, e foi dividido por responsabilidade —
  `partes` (como nasce e por onde é alimentado), `agente` (os fatos do agente) e `pedidos` (a
  fronteira com a interface).

O que **não** deu para verificar nesta máquina: o lançamento entre sessões como SYSTEM (precisa
de elevação) e a passagem de entrada de ponta a ponta (precisa de duas máquinas). São os dois
itens do primeiro teste físico.

## O que a conferência do pacote revelou

Conferir o conteúdo do RPM — e não só o fato de ele ter saído — pegou um defeito que teria
estragado o primeiro teste: **o pacote do Linux levava apenas a interface**. O `%build` compilava
só `inputremote-ui`, e instalar aquele RPM na máquina Linux entregaria a janela em modo de
demonstração, sem serviço nenhum. O spec agora compila e instala o serviço, e traz uma unidade
`systemd` (que roda como root, porque é quem tem `/dev/uinput`). O agente não entra no Linux, e
isso é o desenho: lá a injeção por `uinput` entra abaixo do compositor, e quem a faz é o próprio
serviço.

Do mesmo tipo, e no mesmo lugar onde ninguém olha: o `LEIAME.txt` que vai **dentro** do MSI
ainda dizia que a janela era uma demonstração, e o `README.md` — que o RPM também empacota —
abria com "Estado: especificação. Nenhuma linha de código de produção foi escrita ainda". Texto
obsoleto dentro de um instalador não é detalhe: é a primeira coisa que quem instala lê, e ele
contradiz o que o programa faz.


**Decisões:** o agente é sem `tokio` — os ganchos já rodam numa thread própria com laço de
mensagens, e o trabalho é bloqueante por natureza; o agente **sai** quando a conexão cai, em vez
de remendar um estado que não é dele; a recusa de injeção é **contada** ao serviço, e não só
registrada, porque do ponto de vista do usuário nada aconteceu e ele não teria como saber.
