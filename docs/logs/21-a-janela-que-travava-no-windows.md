# A janela que travava no Windows, e o pareamento que nunca fechava

**Data:** 2026-09-15

**Itens:** nenhum item novo fecha; é correção de defeito encontrado no teste físico Windows → Linux.
Commit da correção: `7c1aea0`.

## O sintoma

Com os dois serviços instalados e no ar — Windows como servidor, o notebook Fedora como cliente — o
mouse não atravessava. Na janela do Windows, "Parear" ficava carregando e não respondia; no Linux,
"Procurar" não achava nada. A suspeita inicial era o serviço do Linux não ter subido.

## O que o estado real mostrou

**O serviço do Linux estava no ar.** Ativo desde o arranque, escutando UDP 52526.

**Nenhum dos dois lados chamava o outro.** Windows (`server`, 52525) e Linux (`client`, 52526)
estavam com `peers = []` e **sem `peer_addr`**; os dois registros diziam *"sem endereço de par;
aguardando conexão de entrada"*. Dois serviços esperando, ninguém discando. E, sem `peer_addr`, o
"Procurar" não tem o que listar: ainda não há descoberta automática, e o candidato oferecido é só o
endereço configurado — o que explica o Linux vazio.

**Quem disca tem de ser o Windows.** O firewall do Windows tinha regras de entrada para os binários
de `target\debug` e `target\release`, mas nenhuma para `C:\Program Files\InputRemote\
inputremote-daemon.exe`, numa rede marcada como Pública. O Linux não conseguiria chamar o Windows;
o contrário funciona, porque a resposta volta como tráfego de saída.

`peer_addr = "10.0.0.135:52526"` foi gravado na configuração do serviço do Windows (cópia da anterior
em `config.toml.antes-do-peer-addr`) e o serviço reiniciado. Na hora ele chamou o notebook, o Noise
fechou, e os dois registros mostraram **o mesmo código de seis dígitos**. O protocolo era compatível
entre os dois builds (`63bea58` no Linux, `0f0bd41` no Windows): entre eles só mudou uma
reorganização de consultas em `ir-session`.

## O pareamento que não fechava

A partir daí o código vencia de 2 em 2 minutos e o Windows discava de novo, com código novo. Numa
das rodadas o Linux registrou `confirmação recebida: sim` — **o notebook confirmou** — e o Windows
não: o código venceu, o Windows recusou, e o ciclo recomeçou.

A janela do Windows estava travada de verdade: `Responding = False`, sem título, **16 ms de CPU na
vida inteira** e as duas threads em `Wait/Executive`. Ela tinha sido reaberta três vezes.

## A causa

A janela lia o canal numa thread e escrevia na principal, as duas sobre um *named pipe* aberto com
`std::fs::File` e duplicado com `try_clone`. Esse handle é **síncrono**, e o Windows serializa toda
E/S de um objeto de arquivo síncrono: enquanto a thread de leitura está dentro de um `ReadFile`
esperando o serviço falar, a escrita da outra thread espera essa leitura terminar. `try_clone` não
ajuda, porque duplica o handle e mantém o mesmo objeto.

Todo pedido da janela, portanto, só saía quando o serviço, por acaso, mandava algo antes:

- **a janela travava ao abrir** — o aperto de mão ficava preso, e o serviço não tinha por que falar;
- **"Parear" e "São iguais" só saíam no próximo aviso**, que no pareamento vem de 2 em 2 minutos,
  depois de o código já ter vencido;
- **o agente tinha o mesmo defeito, e pior**: no servidor, a thread de captura escreve cada
  movimento do mouse enquanto a principal espera comandos. O movimento só saía quando o serviço
  mandava algum comando. Mesmo pareado, o mouse não teria atravessado. O "agente pronto" aparecia
  no registro só porque o anúncio é escrito antes de a leitura começar.

No Linux nada disso acontece — leitura e escrita num socket Unix não se bloqueiam —, e foi por isso
que a confirmação do notebook chegou. O teste de reconexão da janela passava no Windows porque usa
socket TCP, que não sofre a serialização.

## A prova, antes de mexer

A primeira versão do experimento **não provou nada**: o grupo de controle, com handle assíncrono,
travou igual ao síncrono. O servidor de mentira nunca lia e tinha buffer zero — qualquer escrita
esperaria o outro lado ler, fosse qual fosse o modo do handle. O que se mediu ali foi o servidor
calado, não a hipótese.

Refeito espelhando o serviço de verdade — buffers de 64 KB, um servidor que lê o tempo todo e nunca
responde:

| Cliente | Resultado |
|---|---|
| handle síncrono **com** leitura pendente | escrita bloqueada; servidor recebeu 0 bytes |
| handle síncrono **sem** leitura pendente | escrita em 290 ms; 4 bytes |
| handle **sobreposto** com leitura pendente | escrita em 295 ms; 4 bytes |

O pipe e o servidor funcionam; o que trava é exatamente a combinação que a janela e o agente usavam.

## A correção

`ir_ipc::cliente::abrir` passa a abrir o canal para quem conecta, e a janela e o agente o usam no
lugar das duas cópias do código defeituoso. No Windows o pipe é aberto com **E/S sobreposta**, pelo
cliente de *named pipe* do `tokio` — o mesmo mecanismo que o serviço já usa do lado dele —, e quem
chama continua recebendo um `Read` e um `Write` bloqueantes: nada mudou na janela nem no agente
além da chamada. No Linux segue o socket Unix.

O `ir-ipc` pode depender do `tokio` só no Windows porque não está na lista de crates puros do
`xtask`, e a janela e o agente já dependiam dele. Nenhuma seta nova de dependência.

## Os testes

- `escrever_nao_espera_a_leitura_pendente`: a regressão do impasse, contra um *named pipe* real.
- `o_handle_sincrono_da_biblioteca_padrao_trava_e_por_isso_nao_e_usado`: o mesmo roteiro com
  `std::fs::File` trava. É a prova de que o primeiro mede o que diz — e, se o Windows um dia deixar
  de serializar, este falha e avisa que a volta ficou possível.

459 testes no workspace, `clippy` e `xtask` limpos; o ramo Linux do cliente compila para
`x86_64-unknown-linux-gnu`.

## A prova no hardware

**A instalação.** O MSI com `7c1aea0` foi instalado por cima às 15:16. O instalador ficou ~10 min
em "Preparing to remove older versions": o próprio registro dele mostra 4 min 55 s parados na
verificação de arquivos em uso — o serviço e o agente antigos, sem janela, segurando os `.exe`, com
o Restart Manager desligado desde o [log 20](20-atualizar-sem-reiniciar-e-o-balanco.md) — e depois
a mesma espera dentro da desinstalação da versão antiga. Só então parou o serviço (15:15:58) e
terminou com código 0. A configuração, com o `peer_addr`, foi preservada.

**A janela.** Medida pela sessão paralela que investigava o mesmo problema:

| Binário | Até responder |
|---|---|
| instalado de 13/09 (`0f0bd41`) | **não respondeu em 45 s**; 0,03 s de CPU, 2 threads |
| `7c1aea0`, compilado da árvore corrigida | 0,32 s; o clique em "Parear um computador" respondeu em 67 de 67 amostras, e o "Procurar" voltou com o candidato |
| `7c1aea0`, **o instalado** | **0,28 s**, título "InputRemote", 14 threads |

**O pareamento fechou**, pela primeira vez entre as duas máquinas: o notebook confirmou às 15:23:19,
o Windows às 15:23:21, os dois gravaram a chave do outro (`par gravado`) e registraram
`sessão estabelecida ... carrier=udp`.

Antes disso, duas recusas saíram do botão "São diferentes" no Windows com os códigos **iguais** nos
dois serviços. A janela do notebook tinha sido reaberta 15 s depois do código e não o mostrava — o
defeito 1 abaixo produzindo, na prática, uma recusa por engano. A segunda recusa veio 1,2 s depois do
código seguinte, o que indica um segundo clique caindo no mesmo lugar quando a tela trocou.

**A travessia não foi provada.** Logo depois de estabelecida, a sessão caiu por `Timeout` em 1,7 s e
entrou num laço: os dois serviços registram `sessão encerrada reason=Timeout will_retry=true` a cada
~200 ms. A rede entrega (0% de perda nos dois sentidos, nenhum descarte no socket do notebook), mas o
notebook está com economia de energia do Wi-Fi ligada e picos de 124 ms no sentido Windows →
notebook. Esse problema é outro, e ficou com a sessão paralela, que o usuário encarregou dele — ver
os logs seguintes.

## Defeitos encontrados no caminho, ainda abertos

1. **Uma janela aberta depois do código não o recebe.** O código vai por aviso uma vez só, e
   `Acompanhar` não repete o pareamento pendente. Na bancada, a janela do notebook conectou 1,4 s
   depois de um código e ficou sem ele até o ciclo seguinte.
2. **O vencimento do código é registrado como `reason="códigos diferentes"`.** Ninguém recusou nada:
   venceu. Diagnosticando pelo registro, a conclusão seria "alguém no meio" — o alarme errado.
3. **O instalador do Windows não cria regra de firewall** para o serviço instalado. Numa rede
   Pública, o Windows não pode ser chamado.
4. **Não há como informar o endereço do par pela janela.** Sem descoberta e sem campo de endereço, o
   primeiro pareamento exigiu editar a configuração como administrador.
5. **Clicar "Parear" com um pareamento automático em curso** dispara uma conexão nova, que reinicia o
   *handshake* e troca o código das duas telas.
6. **`erro de criptografia: o handshake seguro falhou`** apareceu no Linux durante uma rediscagem
   (17:47:57 UTC), logo antes de um código novo. Possivelmente um pacote da tentativa anterior —
   investigar.
7. **A conferência de elevação no pareamento não existe ainda**: a janela se declara `Elevado` e o
   serviço não confere, divergindo de [04, §5](../04-seguranca.md). Já estava anotado no código.
8. **Uma leitura presa não termina quando a janela descarta o canal** com o serviço vivo: a thread
   de leitura — e, agora, o runtime do cliente — ficam até o serviço fechar o pipe. Já era assim com
   o handle síncrono.
9. **A camada de rede abandona o handshake de pareamento antes do prazo do pareamento**: em ~105–110 s
   (`handshake falhou` às 15:18:25 e 15:22:57), contra 120 s do ator e os "2 minutos" que a tela
   promete. O outro lado continua preso ao código até o prazo dele e recusa as rediscagens nesse
   intervalo.
10. **A sessão não firma sobre o Wi-Fi da bancada**: cai em `Timeout` e se reinicia a cada ~200 ms
    depois do pareamento. Encaminhado à investigação da sessão paralela.
11. **O `empacotar.ps1` procurava os binários num `target\release` fixo**: compilando com
    `CARGO_TARGET_DIR` em outro disco, o MSI sairia em silêncio com os binários **antigos** — os que
    travam. Apontado pela sessão paralela; corrigido junto com este log.
12. **Os botões da interface não são acessíveis**: feitos de `Rectangle` + `TouchArea`, não declaram
    `accessible-role` nem ação padrão, e não expõem `InvokePattern` a leitor de tela e automação.
    Apontado pela sessão paralela.
13. **O nome do notebook chega como "computador"**: o nome da máquina Linux cai no nome genérico.
