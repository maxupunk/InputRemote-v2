# O motor de transferência, e o nível a mais que a árvore ganhava

**Data:** 2026-09-17

**Itens:** Etapa 8 — `ir-files`. Progresso e cancelamento, em parte.

**O que foi feito:** o crate que os documentos previam desde o começo e que ainda não existia. Ele
é o motor do canal 5: monta o manifesto, produz e consome as mensagens, confere cota e caminho,
escreve em disco e publica. Não cifra e não conhece socket — quem leva os bytes é o
[`ir-net::bulk`](30-o-canal-de-dados-em-tcp.md).

## A forma: puxar, não empurrar

A decisão que organiza o crate inteiro. `Envio::proxima()` responde "a próxima mensagem é esta", e
quem chama decide quando pedir a seguinte — o que na prática é quando o socket aceitou a anterior.

A alternativa seria o motor empurrar blocos numa fila. Numa transferência de 5 GB, essa fila cresce
até a memória acabar sempre que a rede for mais lenta que o disco, e ela costuma ser. Puxando, o
disco nunca vai à frente da rede, e a contrapressão sai de graça — sem limite de fila para calibrar
e sem o impasse que duas filas cheias produzem.

É também o que mantém a seta de dependência de pé: `ir-files` depende de `ir-proto` e de mais nada.
Ele não sabe que existe TCP.

## O defeito que o primeiro teste de travessia pegou

O usuário copia a pasta `relatório`. O manifesto descreve `relatório`, `relatório/a.pdf`. A
montagem fica com essa árvore dentro dela. Publicar renomeando a montagem para
`recebidos/relatório` produz:

```text
recebidos/relatório/relatório/a.pdf
```

Um nível a mais, que o usuário vê e não entende. Não é um caso de borda — é **todo** caso de copiar
uma pasta.

A correção é publicar a entrada de dentro, e não a montagem: quando o manifesto tem uma raiz só, é
ela que é renomeada para o destino, e a casca da montagem sobra vazia para o `Drop` recolher. O
efeito colateral é bom: copiar um arquivo solto agora produz um arquivo solto, e não uma pasta com
um arquivo dentro.

A decisão virou módulo próprio, `publicacao.rs`, porque é a única parte da recepção que o usuário
enxerga.

## As garantias, e onde cada uma mora

| Garantia | Onde | Como se sabe |
|---|---|---|
| Nada escrito fora do destino | `staging::caminho_seguro` | a regra vive no `ir-proto`, conferida **de novo** na hora de escrever |
| Nada além do que foi aceito | `recepcao::escrever` | cada bloco conferido contra o tamanho declarado do item |
| Conteúdo íntegro | BLAKE3 por item | calculado enquanto se escreve, sem segunda leitura |
| Nem árvore parcial nem temporário | `Drop` de `Staging` | apagar é o que acontece quando não se faz nada |
| Publicação sem meio-caminho | um `rename` | antes dele não há nada no destino; depois, está tudo |

A segunda linha é a que protege a cota de verdade. Sem ela, um par anuncia um byte e manda
gigabytes — a aprovação do manifesto não protegeria nada.

A quarta é o critério de saída da etapa, e está no `Drop` de propósito: caminho de erro é
exatamente onde a chamada de limpeza é esquecida.

## Recusar e derrubar são respostas diferentes

A distinção acabou organizando o código e os testes, em dois arquivos separados.

**Recusar** é resposta legítima a pedido legítimo — não há cota, não há permissão, não há disco, o
caminho não é seguro. Vira um `Reject` com motivo, o par entende, e o enlace continua. Nenhuma
recusa toca o disco.

**Derrubar** é o que se faz quando o outro lado diz o que não pode ser verdade: bloco maior que o
tamanho que ele mesmo declarou, deslocamento fora de ordem num transporte que garante ordem, fim de
um arquivo que nunca começou, item fora do manifesto. Não há resposta cortês a dar.

Duas consequências que não são óbvias e estão fixadas por teste:

- **Um total de manifesto que não bate com a soma dos itens derruba o enlace**, em vez de virar
  recusa. Não há `RejectReason` para isso, e é certo que não haja: significa que o codificador do
  par está quebrado.
- **Mensagem de outra transferência é ignorada**, e não derrubada. Pode ser sobra de uma cópia que
  o usuário já substituiu, e o par tinha o direito de ter mandado antes de saber. Derrubar por isso
  transformaria copiar duas vezes rápido numa queda de conexão.

## Três decisões sobre o que não entra no manifesto

**Vínculo simbólico não é seguido nem incluído.** Seguir abre dois problemas: um laço faz a
varredura não terminar, e um vínculo para fora copia o que o usuário não selecionou — `~/.ssh`, se
alguém puser um atalho numa pasta copiada. A contagem de ignorados é devolvida para a interface
poder dizer.

**Nome que não é UTF-8 não entra.** O campo do fio é `String`. Uma substituição silenciosa criaria
arquivo com nome diferente do original no destino.

**Retomada não existe.** Se o resumo não conferir ou o enlace cair, a árvore vai embora e a cópia é
refeita. Retomar exigiria confiar num estado parcial em disco entre duas execuções, e a garantia
pedida foi cópia **certa**, não cópia rápida na segunda tentativa.

## Um erro com nome próprio

Se um arquivo mudar de tamanho entre o manifesto e a leitura, o envio falha com
`MudouDurante`, e não com um resumo divergente do outro lado. A diferença importa: "resumo
divergente" aponta para corrupção de transporte, quando a causa foi o usuário salvando o arquivo no
meio da cópia. Diagnóstico errado é pior que diagnóstico nenhum.

## Arquivos

`crates/ir-files/` inteiro, novo: `cota`, `manifesto`, `envio`, `recepcao`, `publicacao`,
`staging`, `error`. Testes em `tests/`: `travessia`, `recusa`, `violacao`, `envio`, e o condutor
compartilhado em `tests/comum/`. Mais `Cargo.toml` do workspace e `PROGRESSO.md`.

**Verificação:** `cargo test --workspace` — 678 testes, 42 suítes, zero falhas; `ir-files` sozinho
tem 69. `cargo clippy --workspace --all-targets` silencioso. `cargo xtask`: limites de tamanho,
setas de dependência e privacidade dos logs, 182 arquivos.

Três arquivos passaram de 400 linhas e foram divididos por responsabilidade, não por conveniência:
`publicacao.rs` saiu da recepção, os testes de `envio` viraram teste de integração — que é onde
quem olha de fora pertence —, e a suíte de recusa se partiu exatamente na linha que o próprio
documento dela desenhava, entre recusar e derrubar.

**O que ainda não foi provado:** nada disto atravessou uma placa de rede. O motor está testado
contra um condutor em memória e o transporte está testado contra `tokio::io::duplex`; ligá-los
dentro do `ir-daemon` é o passo seguinte, e é o que falta para o Ctrl+C e o Ctrl+V do usuário
funcionarem. Os 5 GB com a entrada intacta continuam sendo medição de bancada.
