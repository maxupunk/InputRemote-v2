# 17 — A janela que volta sozinha, e o grupo que vale na hora

**Data:** 2026-09-11

**Itens:** "A janela reconecta ao serviço sozinha" fechou; "Cada canal declara quem pode abri-lo"
mudou de mecanismo no Linux — da permissão do arquivo para a credencial de quem conecta.

## Os dois defeitos

Os dois ficaram abertos no [log 16](16-o-servico-trancou-a-propria-janela.md), e os dois tinham a
mesma forma: uma decisão tomada **uma vez só**, num momento em que a informação ainda não era a
certa.

**A janela não reconectava.** O `main` escolhia, ao abrir, entre o serviço de verdade e o simulado
— e ninguém cuidava da ligação depois. Uma atualização de pacote reiniciava o serviço e a janela
ficava sem conexão até ser fechada. Pior: se ela abria com o serviço parado, caía no simulado e
mostrava dado inventado para sempre, mesmo depois de ele subir.

**O menu abria a janela sem acesso.** O socket de controle era `0660 root:inputremote`, e a
permissão de arquivo é conferida com os grupos **que o processo carrega**. No GNOME a sessão
gráfica nasce do `systemd --user`, que sobrevive ao logout e guarda os grupos de quando nasceu. O
`usermod` estava certo no banco de usuários e não chegava à janela até a máquina ser reiniciada.

## O desenho

Uma responsabilidade por peça, e as dependências apontando para abstrações:

| Peça | Faz | Não sabe |
|---|---|---|
| `ir-ui::conector` | abrir o canal da plataforma (*pipe* ou socket Unix) | quando, nem quantas vezes |
| `ir-ui::conexao` | perceber a queda, esperar, reconectar, refazer a inscrição nos avisos | como se abre um canal, o que a janela mostra |
| `ir-ui::real` | apresentar a conexão como o `Servico` que a janela usa | nada — é adaptador |
| `ir-daemon::ipc::porteiro` | a regra de quem entra em cada canal, **pura** | sistema operacional |
| `ir-daemon::ipc::grupo` | consultar o banco de usuários na hora (`getpwuid_r`, `getgrouplist`) | quem pode entrar |
| `ir-daemon::ipc::escuta` | aceitar e devolver a conexão **com a decisão** | a regra em si |

A conexão recebe o conector e o intervalo de fora, e é isso que deixou a reconexão testável inteira
sem serviço, sem *pipe* e sem dormir nos testes. O porteiro é uma função pura pelo mesmo motivo: a
parte que precisa estar certa é a única que dá para testar por inteiro sem montar usuários de
verdade.

Três escolhas que valem registrar:

- **A recusa é informação, não silêncio.** Fechar o canal mudo deixaria a janela sem distinguir
  "serviço parado" de "sem permissão" — e as duas pedem ações sem nada em comum. O serviço responde
  ao primeiro pedido com `Falha::SemPermissao` e só então fecha, dentro do pedido-resposta que a
  janela já espera.
- **O contrato só cresce no fim.** `SemPermissao` e `ServicoIndisponivel` entraram depois de
  `Interna`, porque o `postcard` grava o índice da variante no fio. Um teste trava o índice de cada
  falha: uma reordenação "cosmética" passaria em todos os outros testes, porque dentro de uma mesma
  versão os dois lados concordam — errado, do mesmo jeito.
- **`Servico::situacao()` substitui `simulado()`.** A faixa amarela deixou de dizer "dados
  simulados" e passou a dizer o que aconteceu e o que fazer, na instrução da plataforma em que a
  janela está. O simulado continua existindo, mas só com `--simulado`: cair nele sozinho mostrava
  dado de mentira a quem só precisava saber que o serviço estava parado.

## Como foi verificado

**Reconexão, no Windows, com *named pipes* reais:** serviço no ar, janela aberta e conectada;
serviço derrubado, a janela continuou viva; serviço novo no ar, a **mesma janela** conectou sozinha
— uma conexão registrada em cada serviço, sem reabrir nada.

**Reconexão, nos dois sistemas, por teste** — contra um serviço de mentira que fala o protocolo real
num socket TCP de *loopback*: sem serviço, o pedido falha com `ServicoIndisponivel` e não com
"falha interna"; o serviço sobe depois da janela e ela entra; a conexão cai e ela volta; a permissão
é recusada e, liberada, ela entra na tentativa seguinte.

**O grupo na hora, num Fedora 44 de verdade, com o RPM desta construção e o protocolo em bytes:**

| Caso | Grupos do processo | Resposta |
|---|---|---|
| usuário no grupo | com o grupo | `02 00 00 00 00 00` — aceita |
| usuário fora do grupo | sem o grupo | `03 00 00 00 00 03 06` — `SemPermissao` (índice 6) |
| processo nascido **antes** do `usermod`, conectando depois | `1001`, **sem** o `999` | `02 00 00 00 00 00` — aceita |

A terceira linha é a pegadinha do GNOME reproduzida: um processo com grupos congelados, cujo próprio
`id -G` não mostra o grupo, e que o serviço aceita porque consulta o banco de usuários. Foi também a
primeira vez que o FFI com a glibc rodou fora de teste de unidade.

Suíte: **450 testes no Windows**; **77 no Linux** em `ir-ipc`, `ir-daemon` e `ir-ui`. `clippy
--all-targets`, `cargo xtask` e `fmt --check` limpos nos dois.

**O que não foi verificado:** a conferência visual no notebook Linux com GNOME — ele estava fora da
rede (suspenso) durante o trabalho. O mecanismo está provado; ver a faixa sumir na tela dele é o que
falta.

## Decisões

- O `ir-ipc/src/ui.rs` passou de 400 linhas com as falhas novas. Em vez de subir o limite, `Falha`
  foi para `falha.rs` com os testes que são dela — é o vocabulário com regra própria do contrato.
  `ir_ipc::Falha` e `ir_ipc::ui::Falha` continuam valendo.
- root e o próprio dono do serviço sempre entram: nenhum dos dois cruza fronteira de privilégio.
- Conexão sem credencial legível é negada. Na dúvida, o canal não abre.
- O socket de controle `0666` não é permissão frouxa: o portão é a credencial, conferida a cada
  conexão. O do agente continua `0600`.
- Recusado por permissão, a janela espera dez vezes mais para tentar de novo: permissão muda na
  velocidade de alguém digitando um comando, e cada tentativa recusada é uma linha no registro do
  serviço.

**Arquivos:** `crates/ir-ui/src/{conector,conexao,real,servico,simulado,janela,main,lib}.rs`,
`crates/ir-ui/ui/{app,dados}.slint`, `crates/ir-ui/tests/{reconexao,interface}.rs`,
`crates/ir-daemon/src/ipc/{porteiro,grupo,escuta,controle,agente,mod}.rs`,
`crates/ir-ipc/src/{falha,ui,lib}.rs`, `USAR.md`, `PROGRESSO.md`.
