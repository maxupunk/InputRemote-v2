# 16 — O serviço trancou a própria janela do lado de fora

**Data:** 2026-09-11

## O sintoma

Instalado e rodando nos dois sistemas, e a janela continuava mostrando a faixa amarela:

> Modo de demonstração: o serviço do InputRemote não está respondendo, e os dados abaixo são
> simulados. Nada é injetado em computador nenhum.

O incômodo aqui não é o defeito — é que **tudo o mais parecia certo**. O serviço estava
`Running`, o agente estava no ar, os dois canais existiam, e os binários instalados eram os do
dia. Nenhum registro de erro, em lugar nenhum.

## O diagnóstico

Olhar o estado em vez de supor foi o que resolveu. O serviço rodava como `LocalSystem`, os
*pipes* `inputremote-control` e `inputremote-agent` existiam, e os três processos estavam vivos.
Então a pergunta deixou de ser "o serviço subiu?" e passou a ser "**a janela consegue abrir o
canal?**". Abrindo o *pipe* à mão, com o usuário comum:

    O acesso ao caminho foi negado.

## A causa

Um *named pipe* criado por um processo `LocalSystem` com o descritor de segurança **padrão** não
dá acesso ao usuário interativo. O canal existia, e era inalcançável para quem mais precisava
dele. No Linux, o mesmo defeito por outro mecanismo: o serviço roda como root (é quem tem
`/dev/uinput`), então o socket nascia `root:root`, e a janela do usuário levava permissão negada.

A parte incômoda: isto estava **escrito** no código, como ressalva — "o *pipe* usa o descritor
padrão, o que basta enquanto serviço e interface rodam sob o mesmo usuário no teste em primeiro
plano". A ressalva era verdadeira, e deixou de valer exatamente quando o produto passou a rodar
do jeito certo, como serviço. Uma anotação de "depois eu vejo" não é um aviso: ela só aparece
quando alguém já está com o problema na frente.

**Um padrão herdado é uma decisão que ninguém tomou.** Se quem pode falar com o serviço importa —
e importa: quem fala com ele consegue digitar na máquina —, então isso é declarado, não herdado.

## A correção

Cada canal declara o próprio acesso, e os dois são diferentes de propósito:

| Canal | Windows (SDDL) | Linux | Quem alcança |
|---|---|---|---|
| controle | `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)` | `0660 root:inputremote` | serviço, administradores e o **usuário interativo** |
| agente | `D:P(A;;GA;;;SY)(A;;GA;;;BA)` | `0600 root` | só o serviço e administradores |

O canal do agente continua fechado porque é ele que carrega injeção de entrada: se um processo
qualquer do usuário pudesse abri-lo, qualquer programa que ele rodasse poderia digitar no prompt
de UAC ([04, §5](../04-seguranca.md)). Abrir os dois "para resolver" teria trocado um defeito de
usabilidade por um buraco de segurança.

No Linux o acesso é por grupo, e não por permissão frouxa: o RPM cria o grupo `inputremote` e
quem for operar a máquina entra nele de propósito — uma decisão registrada do administrador.

## Como foi verificado

Com o serviço em primeiro plano e o teste feito **como usuário comum, não elevado**:

- `CONTROLE -> CONECTOU` — a janela alcança o serviço;
- `AGENTE -> NEGADO (O acesso ao caminho foi negado)` — o canal de injeção segue fechado.

A prova foi repetida no binário de **release** — o que de fato vai dentro do instalador —, e não
só no de depuração. Provar no artefato errado é o tipo de engano que passa despercebido
justamente porque o resultado vem verde.

Dois enganos apareceram no caminho e vale registrar os dois, porque ambos produziram "prova"
falsa:

- a primeira tentativa de prova rodou com o serviço instalado ainda no ar, e o serviço de teste
  morreu em `os error 10048` (porta UDP já em uso). O *pipe* nunca chegou a existir, e o
  resultado apareceu como "acesso negado" — um erro lido como confirmação do outro;
- o teste do canal do agente passou a falhar **por estar certo**: ele conecta como usuário comum,
  e o descritor restrito nega. O teste é que estava errado, não o código. Ele agora exercita a
  lógica do canal e deixa o descritor para quem o prova de verdade.

## O grupo que não chegava na sessão

Na máquina Linux de verdade apareceu mais uma camada, e ela contradizia a instrução que eu tinha
escrito. Com o serviço certo, o socket `root:inputremote` e o usuário já no grupo, a pessoa **saiu
e entrou na sessão** — e a janela continuou sem acesso. Olhando os grupos de cada processo:

| Processo | Tem o grupo? |
|---|---|
| `systemd --user`, iniciado antes do `usermod` | não |
| `gnome-shell` da sessão **nova**, filho dele | não |
| a janela, filha do `gnome-shell` | não |
| uma sessão SSH nova | sim |

No GNOME, os programas da sessão gráfica nascem do gerenciador `systemd --user`, e ele
**sobrevive ao logout** enquanto existir qualquer outra sessão do mesmo usuário — inclusive a
conexão SSH usada para diagnosticar. Ele carrega os grupos de quando nasceu. "Saia e entre", a
instrução clássica para grupo novo, simplesmente não vale ali; só reiniciar (ou `sg inputremote`)
resolve.

Duas lições. A primeira é de diagnóstico: `id` num terminal novo mostrava o grupo e dizia que
estava tudo certo — o que importa é o grupo **do processo que abre o socket**, lido em
`/proc/<pid>/status`. A segunda é de desenho: acesso por grupo tem essa aresta afiada no desktop
moderno. Serve para agora, mas conferir a credencial do processo que conecta (`SO_PEERCRED`) no
próprio serviço dispensaria o grupo e essa pegadinha junto — fica anotado como a forma mais robusta.

## Duas coisas que só a máquina real mostrou

**"Instalei e continua igual."** O RPM saía sempre com a mesma versão-release, e `dnf install`
sobre uma NEVR já instalada não faz nada — sai com sucesso e deixa o pacote velho no lugar. Cada
construção agora leva um carimbo no release, e na máquina real o `dnf install` passou a atualizar
de fato (`...135531` → `...144504`). Falha silenciosa com cara de sucesso é a pior categoria, e
esta estava no instalador.

**A janela não reconecta.** Ao atualizar o pacote, o serviço reinicia e a janela aberta perde a
conexão para sempre: ela só conecta ao abrir. Pior, se abriu com o serviço fora do ar, escolhe o
simulado e nunca mais tenta. Ficou anotado como item aberto, e **não** foi corrigido no meio do
teste físico — mexer no único caminho que tinha acabado de funcionar, com a pessoa olhando a tela,
trocaria um defeito conhecido por um risco desconhecido.

**Decisões:** o `unsafe` da criação do *pipe* com descritor ficou confinado ao módulo
`seguranca`, e não vazou para o `escuta`, que é código comum; no Linux escolheu-se grupo em vez
de `0666`, porque enquanto a exigência de elevação não for imposta no transporte, permissão
frouxa deixaria qualquer usuário local conduzir um pareamento.
