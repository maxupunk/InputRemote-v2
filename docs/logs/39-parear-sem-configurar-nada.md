# Parear sem configurar nada: a descoberta que não existia, e o mDNS que o Windows não deixava usar

**Data:** 2026-09-19

**Itens:** Etapa 4 — descoberta na rede local; Etapa 9 — endereço pela janela; defeitos #1, #3 e #4
do [log 21](21-a-janela-que-travava-no-windows.md).

**O que foi feito:** com os dois computadores recém-instalados, o Windows listava o Linux e, clicando
em "Parear", ficava em "Aguardando o outro computador…" para sempre; o Linux dizia "Nenhum computador
encontrado".

## As causas, pelos registros dos dois serviços

1. **Não havia descoberta.** "Procurar" devolvia só o `peer_addr` do `config.toml`. O Windows
   "achava" o Linux porque o arquivo dele — o `ProgramData` sobrevive à desinstalação — ainda tinha
   `peer_addr = "10.0.0.135:52526"`, da bancada. O Linux novo escutava na 52525.
2. **O pedido que não chegava não dizia nada.** A primeira mensagem do handshake saía uma vez,
   esperava 1,5 s e desistia numa linha de registro. O prazo de dois minutos só existia depois do
   código.
3. **Quem recebe não via o código.** O serviço aceitava o pareamento, mas a janela não ia sozinha
   para a comparação; entrar em "Parear" apagava o código; uma janela aberta depois nunca o recebia.
4. **O Windows não aceitava entrada** numa rede Pública: o instalador não criava regra de firewall.

## O que mudou

**Descoberta própria, por broadcast** (`ir_net::descoberta`, porta 52524/UDP): cada serviço responde
a "quem está aí?" com id, nome e porta; quem procura pergunta ao broadcast de cada sub-rede. A lista
de "Parear" junta isso, os dispositivos Bluetooth pareados no sistema e o endereço da configuração
(`ir_transporte::descoberta`), e se atualiza sozinha a cada 5 s enquanto está aberta.

**Por que não mDNS**, que foi a primeira tentativa: o Linux anunciou (o `avahi-browse` viu), o Windows
não. O serviço do Windows só abriu o socket mDNS IPv6. A 5353 em IPv4 já estava aberta pelo Chrome e
pelo Quick Share na conta do usuário, e o Windows não deixa um processo de **outra conta** — o
serviço é SYSTEM — compartilhar a porta. O mesmo código rodando como o usuário abriu o IPv4. Numa
porta só do produto não há disputa, e a resposta, vindo do endereço que alcança quem perguntou,
dispensa escolher entre os endereços do WSL, do Hyper-V e do Docker. O `mdns-sd` saiu do projeto.

**Handshake de pareamento que espera**: o iniciador reenvia a mesma mensagem a cada 1,5 s por até
12 s; quem responde reenvia a resposta se a pergunta chegar de novo. No Windows, um envio a uma porta
sem ninguém faz o **próximo** `recv_from` falhar com `WSAECONNRESET` — era isso que derrubava o
handshake na hora; agora é ignorado, ali e no laço do endpoint.

**Fim garantido e com motivo**: o serviço marca a discagem (`actor/discagem.rs`) e, com erro daquele
portador ou 20 s sem código, avisa `Aviso::PareamentoFalhou(Falha::ParNaoRespondeu)` — "abra o
InputRemote no outro computador… ou digite o endereço".

**Quem recebe vai para os dígitos**: o código leva a janela à comparação, e a tira da bandeja; entrar
em "Parear" não apaga um código em comparação; e o serviço reconta o código pendente a toda janela
que começa a acompanhar.

**Endereço digitado**: "Não aparece? Digite o endereço" abre um campo (IP, IP:porta ou Bluetooth).

**Firewall**: o MSI cria a regra de entrada do serviço (sub-rede local, por programa); o RPM traz o
serviço `inputremote` do firewalld, que o ajudante de ativação liga.

**Nome da máquina no Linux**: vinha de `HOSTNAME`, que o systemd não define — o par aparecia como
"computador". Agora vem de `/proc/sys/kernel/hostname`.

## A prova

Testes novos: a junção dos candidatos; o formato da pergunta e da resposta (inclusive resposta
truncada e forjada); a busca achando um respondedor de verdade em loopback; o handshake com o outro
lado subindo 3 s depois (antes, falhava em 1,5 s) e desistindo no prazo, e não na hora; a discagem
que falha com motivo, que não confunde portadores, que termina com o código e que vence no prazo; o
código recontado para a janela que chega depois; o endereço digitado.

Na bancada, com o Linux já no pacote novo: a busca dele listou o Windows pelo Bluetooth; o Windows,
ainda na versão anterior, não respondia à pergunta nova. O resultado com os dois atualizados vai no
próximo registro. A lista do Windows, antes do filtro, trazia fones, alto-falantes e teclados junto
com o Fedora — o próximo passo é mostrar só computadores.

**Verificação:** 828 testes no Windows; `verificar.sh` verde no container do Fedora; clippy e `xtask`.
