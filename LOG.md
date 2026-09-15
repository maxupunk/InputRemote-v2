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
