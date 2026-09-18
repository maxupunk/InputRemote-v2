# O serviço instalado como origem, e a recusa pelo motivo certo

**Data:** 2026-09-18

**Itens:** Etapa 8 — o serviço do Windows (SYSTEM) sabe quem pediu o envio: o que o
[log 35](35-o-texto-pelo-canal-4.md) deixou sem provar com o serviço instalado.

**O que foi feito:** MSI e RPM gerados do commit `c92e7bf` (o container do RPM já com teto de CPU e
memória, sem travar a máquina) e instalados nas duas máquinas. O ajudante de clipboard do Windows
ficou registrado em `HKLM\...\Run`; o do Linux, em `/etc/xdg/autostart`.

## A prova

Com o serviço **instalado**, como SYSTEM, em sessão com o Fedora por UDP:

| Cenário | Resultado |
|---|---|
| Pasta com 2 arquivos (300 048 B) no clipboard do Windows | enviada e conferida em 96 ms; SHA-256 dos dois igual no Linux; o ajudante do Linux publicou a pasta |
| Arquivo cuja ACL deixa só o SYSTEM ler, no clipboard do usuário | recusado: "sem permissão para enviar"; nada saiu |

O segundo cenário é o que importa. O serviço **consegue** ler o arquivo; quem não consegue é o usuário
que copiou. A recusa veio da pergunta feita ao Windows com o token dele (`AccessCheck`), e não de
falha ao abrir.

Uma tentativa anterior com a colmeia `SAM` do registro não serve de prova: foi recusada porque o
arquivo está travado pelo sistema (erro 32), antes de a permissão ser perguntada. Fica registrada para
ninguém tomá-la por prova.

## Observado, e aberto

- Uma cópia recusada é oferecida **duas vezes**: o Windows avisa a mudança do clipboard duas vezes, a
  recusa libera a mesma cópia para ir de novo (`Eco::oferta_falhou`), e o segundo aviso a reoferece.
  Inofensivo — a segunda também é recusada —, mas é trabalho à toa e uma linha a mais no registro.
- Por SSH, a leitura do clipboard do GNOME voltou vazia depois de o ajudante publicar; sem o
  inibidor da proteção de tela da bancada, é a limitação já registrada no log 34, não defeito.

**Verificação:** a tabela acima, na bancada; registros do serviço nas duas máquinas.
