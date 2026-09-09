# 10 — Testes e validação

## 1. Pirâmide

| Nível | Onde | Roda em | Quando |
|---|---|---|---|
| Unidade pura | `ir-proto`, `ir-session`, `ir-geometry` | qualquer máquina, sem periférico | todo commit |
| Integração simulada | dois `Session` conversando por transporte falso | qualquer máquina | todo commit |
| Integração de plataforma | backends reais, uma máquina só | Windows e Linux no CI | todo commit |
| Estresse | duas máquinas, tráfego real | bancada física | antes de cada etapa fechar |
| Validação física | roteiro humano, quatro combinações | bancada física | antes de cada lançamento |

A base é larga de propósito: o desenho *sans-io* de [ADR-0004](adr/0004-nucleo-sans-io.md)
existe para que a maior parte do produto seja testável sem hardware. No v1, quase nada
podia ser testado sem dois computadores e um rádio.

## 2. Testes do núcleo

`ir-session` é testado como uma função: entra uma sequência de `Input`, sai uma sequência
de `Command` esperada. Cenários obrigatórios:

| Cenário | O que se verifica |
|---|---|
| Travessia de borda ida e volta | `EnterScreen`/`LeaveScreen`, supressão local ligada e desligada |
| Enlace cai com 3 teclas pressionadas | `ReleaseAll` emitido antes de qualquer outra coisa |
| Enlace cai e volta | reconexão sem novo pareamento, `StateSnapshot` reenviado |
| Troca de portador com o controle no par | nenhum evento duplicado, nenhum perdido |
| Agente perdido e recuperado | `StateSnapshot` reenviado, teclas reconciliadas |
| Snapshot divergente do estado local | reconciliação idempotente, nas duas direções |
| Atalho de emergência | controle volta, tudo liberado, mesmo com o enlace vivo |
| Par some no meio da travessia | controle volta em ≤ 1 s de tempo simulado |
| Geometrias diferentes nos dois lados | posição de entrada correta em todas as bordas |
| Monitor removido durante a sessão | sem coordenada fora de tela, sem pânico |

Cobertura mínima de 85% em `ir-session` e `ir-proto`, com piso verificado no CI.

## 3. Fuzzing

`cargo fuzz` contra o decodificador do `ir-proto` e contra a máquina de estados, com
corpus versionado. Roda no CI noturno e antes de todo lançamento.

Alvos:

1. decodificação de mensagem a partir de bytes arbitrários;
2. sequência arbitrária de `Input` no `Session` — não deve haver pânico nem estado
   inválido alcançável;
3. quadro Noise adulterado, truncado e repetido.

Isto não é excesso de zelo: o decodificador roda como `SYSTEM`, recebendo bytes de um
rádio aberto, antes de qualquer login ([04, §1](04-seguranca.md)).

## 4. Testes de plataforma

**Windows.** Serviço instalado numa VM do CI; agente lançado em `Default`; injeção
verificada por Raw Input num processo testemunha; troca de desktop simulada por
`Win+L` via `LockWorkStation`.

O que só pode ser verificado em máquina física, e portanto entra na §6: injeção efetiva
no desktop `Winlogon`, `SendSAS` e o comportamento sob UAC.

**Linux.** Contêiner privilegiado com `/dev/uinput`; criação dos dispositivos verificada
pelo `libudev`; injeção verificada lendo `/dev/input/eventN` de volta; portais testados
contra um GNOME sem monitor (`headless`), com o compositor rodando no CI.

## 5. Estresse

| Teste | Duração | Critério |
|---|---|---|
| 10.000 travessias de borda | ~2 h | zero teclas presas, zero botões presos |
| Digitação contínua a 15 teclas/s | 1 h | zero perdas no canal confiável |
| Ponteiro a 1.000 Hz | 30 min | mediana e p99 dentro da meta, sem crescimento de memória |
| Transferência de 5 GB durante uso | — | latência de entrada piora no máximo 10% |
| Queda de rádio a cada 30 s | 1 h | reconexão sempre em ≤ 5 s, zero teclas presas |
| Perda de 5% e reordenação em UDP | 1 h | zero teclas presas; sessão cai em vez de agir com lacuna |
| Serviço morto à força a cada 60 s | 1 h | teclas liberadas sempre, sem estado corrompido |
| Repouso do serviço | 24 h | memória residente estável, sem crescimento |

## 6. Roteiro de validação física

Quatro combinações — Windows→Windows, Windows→Linux, Linux→Windows, Linux→Linux — vezes
dois portadores. Cada uma percorre:

1. instalar nos dois lados; o serviço sobe no boot;
2. parear: código de seis dígitos igual nas duas telas, confirmar nos dois lados;
3. atravessar a borda nas quatro direções, com dois monitores de escalas diferentes;
4. digitar em campo de texto com acento, `AltGr` e teclas mortas;
5. copiar texto, imagem e uma pasta, nos dois sentidos;
6. **bloquear o cliente e digitar a senha pelo teclado do servidor** — N2, obrigatório;
7. **provocar um prompt de UAC no cliente e operá-lo** — N2, obrigatório;
8. **reiniciar o cliente e digitar a senha na tela de login, antes de qualquer sessão** —
   N3; se falhar, registra-se o nível N2 para aquela plataforma e o roteiro **continua**;
9. Ctrl+Alt+Del pelo teclado do servidor, com a política habilitada;
10. desligar o rádio no meio do uso e observar a degradação anunciada;
11. suspender e retomar as duas máquinas;
12. usar o atalho de emergência com o controle no par;
13. desinstalar dos dois lados e confirmar que não sobrou resíduo.

Os passos 6 e 7 são o produto: se falharem, não há versão. O passo 8 é o alvo: se falhar,
a plataforma é declarada N2 e a versão sai assim, com a limitação no README e na interface
([01, §2](01-visao-e-escopo.md)).

O nível alcançado por plataforma é registrado junto com o resultado, e uma versão nunca
sai com um nível **menor** que o da versão anterior sem que isso esteja escrito nas notas
de lançamento. Regressão silenciosa de capacidade é a pior coisa que pode acontecer com
este produto.

## 7. Como a latência é medida

Sem método definido, "mediana menor que 20 ms" é opinião.

1. os relógios monotônicos das duas máquinas são alinhados por 200 trocas de `Ping`/`Pong`,
   guardando o menor RTT observado e a diferença estimada;
2. no servidor, cada evento recebe carimbo no instante em que sai do gancho ou do `libei`;
3. no cliente, o carimbo é anotado imediatamente antes da chamada de injeção;
4. a diferença, corrigida pelo desvio estimado, é a **latência adicionada** — não inclui
   o atraso do próprio periférico nem o do compositor, que não são nossos;
5. a amostra mínima é de 10.000 eventos, e o relatório traz mediana, p95, p99 e máximo;
6. o resultado é gravado em `bench/` com a data, o hardware e as versões dos dois lados.

Comparações entre versões só valem no mesmo hardware. Uma medição sem esse registro não
entra em decisão nenhuma.

## 8. O que o CI roda

| Gatilho | O quê |
|---|---|
| Todo push | `fmt`, `clippy -D warnings`, testes, `deny`, `xtask check-limits`, `check-deps`, `check-logs` |
| Todo push | testes de plataforma no Windows e no Linux |
| Todo push | *benchmark* de regressão do caminho quente |
| Noturno | `cargo fuzz` com orçamento de tempo, cobertura, auditoria de dependências |
| Tag de lançamento | tudo acima, mais empacotamento, assinatura e `.sha256` |

Nenhum resultado é aviso. Ou passa, ou a build falha. Aviso é ruído que se aprende a
ignorar, e foi assim que o `ROADMAP.md` do v1 chegou ao lançamento com itens em aberto.
