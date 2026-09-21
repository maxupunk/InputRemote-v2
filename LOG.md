# Log de implementação

Índice das entradas. **Um arquivo por tópico**, em [`docs/logs/`](docs/logs/), numerado na ordem
em que o trabalho aconteceu.

Entrada nova ganha arquivo novo com o próximo número, e uma linha aqui. Nada é editado depois de
escrito: se algo se provou errado, a correção vira uma entrada nova que diz o que mudou e por
quê. Um registro que se reescreve não é registro.

## Formato de cada arquivo

```markdown
# <título curto>

**Data:** AAAA-MM-DD

**Itens:** <quais itens do PROGRESSO fecharam>
**O que foi feito:** <descrição>
**Arquivos:** <lista>
**Verificação:** <o comando que rodou e o resultado, ou a inspeção feita>
**Decisões:** <o que foi decidido no caminho, se houver>
```

## Entradas

| # | Data | Tópico |
|---|---|---|
| 01 | 2026-09-09 | [Fundação do repositório](docs/logs/01-fundacao-do-repositorio.md) |
| 02 | 2026-09-09 | [`ir-proto`: o núcleo do protocolo, puro e testado](docs/logs/02-ir-proto.md) |
| 03 | 2026-09-09 | [`ir-geometry`: telas, bordas e mapeamento de coordenadas](docs/logs/03-ir-geometry.md) |
| 04 | 2026-09-09 | [`ir-session`: a máquina de estados do produto](docs/logs/04-ir-session.md) |
| 05 | 2026-09-09 | [Confiabilidade sobre datagrama, e sete defeitos que ela revelou](docs/logs/05-confiabilidade-sobre-datagrama.md) |
| 06 | 2026-09-09 | [`xtask`: as regras de docs/09 viram executáveis](docs/logs/06-xtask.md) |
| 07 | 2026-09-09 | [PoC-1: o spike da tela de bloqueio, pronto para rodar](docs/logs/07-poc1-tela-de-bloqueio.md) |
| 08 | 2026-09-09 | [`ir-ipc`: o contrato que a interface enxerga](docs/logs/08-ir-ipc.md) |
| 09 | 2026-09-09 | [A interface: três telas, e o desenho no lugar da pergunta](docs/logs/09-interface.md) |
| 10 | 2026-09-10 | [Empacotamento: o que dá para instalar hoje](docs/logs/10-empacotamento.md) |
| 11 | 2026-09-10 | [Instaladores: MSI, RPM e assinatura, num comando](docs/logs/11-instaladores.md) |
| 12 | 2026-09-10 | [O ícone, e a janela preta que abria atrás](docs/logs/12-icone-e-janela-sem-console.md) |
| 13 | 2026-09-10 | [A pilha completa: do pareamento cifrado à sessão de pé](docs/logs/13-pilha-completa-mouse-cruzando.md) |
| 14 | 2026-09-10 | [O serviço de ponta a ponta: IPC, pareamento pela janela e o serviço que sobe](docs/logs/14-servico-de-ponta-a-ponta.md) |
| 15 | 2026-09-10 | [O agente de sessão: o serviço alcança a sessão do usuário](docs/logs/15-agente-de-sessao.md) |
| 16 | 2026-09-11 | [O serviço trancou a própria janela do lado de fora](docs/logs/16-o-servico-trancou-a-propria-janela.md) |
| 17 | 2026-09-11 | [A janela que volta sozinha, e o grupo que vale na hora](docs/logs/17-a-janela-que-volta-e-o-grupo-que-vale-na-hora.md) |
| 18 | 2026-09-11 | [A prova no notebook, e a troca de papel que ninguém viu](docs/logs/18-a-prova-no-notebook-e-a-troca-de-papel.md) |
| 19 | 2026-09-11 | [A troca de papel e de borda que vale na hora](docs/logs/19-a-troca-que-vale-na-hora.md) |
| 20 | 2026-09-11 | [Atualizar sem reiniciar, e o balanço do que falta](docs/logs/20-atualizar-sem-reiniciar-e-o-balanco.md) |
| 21 | 2026-09-15 | [A janela que travava no Windows, e o pareamento que nunca fechava](docs/logs/21-a-janela-que-travava-no-windows.md) |
| 22 | 2026-09-15 | [A sessão que reiniciava a cada 200 ms](docs/logs/22-a-sessao-que-reiniciava-a-cada-200-ms.md) |
| 23 | 2026-09-15 | [O pico de latência que virava queda](docs/logs/23-o-pico-de-latencia-que-virava-queda.md) |
| 24 | 2026-09-15 | [A borda é do servidor](docs/logs/24-a-borda-e-do-servidor.md) |
| 25 | 2026-09-15 | [O pareamento que se desfazia depois do clique](docs/logs/25-o-pareamento-que-se-desfazia-depois-do-clique.md) |
| 26 | 2026-09-15 | [O Bluetooth que não existia, e o portador que o serviço ignorava](docs/logs/26-o-bluetooth-que-nao-existia.md) |
| 27 | 2026-09-16 | [O par que voltava sozinho, e a latência que eu li errado](docs/logs/27-o-par-que-voltava-sozinho.md) |
| 28 | 2026-09-16 | [A carga que refutou o *sniff*, e o rádio que ficava ocupado](docs/logs/28-a-carga-que-refutou-o-sniff.md) |
| 29 | 2026-09-16 | [O agente que nunca chegava a dizer por quê](docs/logs/29-o-agente-que-nunca-dizia-por-que.md) |
| 30 | 2026-09-17 | [O canal de dados em TCP, e o teto que o Noise nunca teria permitido](docs/logs/30-o-canal-de-dados-em-tcp.md) |
| 31 | 2026-09-17 | [O motor de transferência, e o nível a mais que a árvore ganhava](docs/logs/31-o-motor-de-transferencia.md) |
| 32 | 2026-09-17 | [Arquivos atravessando, e a fronteira que o limite de linhas cobrou](docs/logs/32-arquivos-atravessando.md) |
| 33 | 2026-09-17 | [O clipboard sem interceptar atalho, e o laço que a guarda desfaz](docs/logs/33-o-clipboard-sem-interceptar-atalho.md) |
| 34 | 2026-09-18 | [Copiar aqui, colar lá — e o serviço que podia ler demais](docs/logs/34-copiar-aqui-colar-la.md) |
| 35 | 2026-09-18 | [O texto pelo canal 4, o alcance da confirmação, e o enlace que só um lado achava vivo](docs/logs/35-o-texto-pelo-canal-4.md) |
| 36 | 2026-09-18 | [O serviço instalado como origem, e a recusa pelo motivo certo](docs/logs/36-o-servico-instalado-como-origem.md) |
| 37 | 2026-09-18 | [A bandeja, o ícone em branco e a rolagem de lado](docs/logs/37-a-bandeja-e-a-rolagem-de-lado.md) |
| 38 | 2026-09-18 | [A senha pedida pela janela, e não um comando de terminal](docs/logs/38-a-senha-pedida-pela-janela.md) |
| 39 | 2026-09-19 | [Parear sem configurar nada: a descoberta que não existia, e o mDNS que o Windows não deixava usar](docs/logs/39-parear-sem-configurar-nada.md) |
| 40 | 2026-09-19 | [O ajudante que ninguém subia, os arquivos que não achavam o par, e as teclas que sumiam](docs/logs/40-o-ajudante-que-ninguem-subia.md) |
| 41 | 2026-09-20 | [A cópia que se repetia, o 0% que não andava, e a janela que não contava nada](docs/logs/41-a-copia-que-se-repetia.md) |
