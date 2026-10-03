# A pasta compartilhada: o contrato antes do motor

**Data:** 2026-10-02

**Itens:** [ADR-0015](../adr/0015-pastas-compartilhadas.md) — pastas compartilhadas;
[03, §6](../03-protocolo.md) — canal 5; [03, §8](../03-protocolo.md) — versionamento;
[02, §2](../02-arquitetura.md) — fronteiras entre crates; [PROGRESSO](../../PROGRESSO.md), Etapa 11, F0.

**O que foi pedido:** uma pasta que existe nos dois computadores, do jeito do OneDrive ou do Google
Drive, para quem não entende de rede. Compartilhar num lado, aparecer no outro; baixar só ao abrir;
editar offline e sincronizar quando voltar, com o IP que for; nos conflitos, o que o mercado faz de
melhor.

## A investigação

O que já existia e serve: o canal 5 é TCP com Noise IK, e já reencontra o par pela identidade da
máquina quando o IP muda ([log 40](40-o-ajudante-que-ninguem-subia.md)). É a parte "sem entender de
rede" pronta.

Quatro coisas no código decidiram a forma:

- **Mensagem desconhecida derruba o enlace** ([03, §8](../03-protocolo.md)). Diferente do manifesto em
  partes da versão 7 — que só saía num manifesto que a 6 já não conseguia mandar —, a pasta acrescenta
  mensagem que um par antigo **receberia**. Daí o portão de versão.
- **A fila do canal 5 faz uma cópia por vez, e a mais nova cancela a anterior** (`fila.rs`). Sincronia
  por ela cancelaria o Ctrl+C do usuário. Faixa própria, no mesmo remetente — que `sessao.rs` já
  compartilha por `Mutex`.
- **O serviço no Linux não escreve em `$HOME`** (`ProtectHome=read-only`), e no Windows escreveria como
  `SYSTEM`. Quem grava é um ajudante novo, como o usuário; o serviço só repassa.
- **O canal de controle não carrega dados**: atende um pedido por vez pelo ator e descarta aviso quando
  quem lê se atrasa (`ir-canais/src/controle.rs`). Daí um terceiro canal local.

As decisões de produto — com o usuário: os dois lados compartilham (origem e réplica por pasta); a
réplica do Windows é sob demanda pela Cloud Files API; a do Linux começa como cópia completa; no
conflito, as duas versões ficam. Registradas no [ADR-0015](../adr/0015-pastas-compartilhadas.md),
com as alternativas rejeitadas.

## O que foi feito

Só o contrato: nada muda no comportamento, e nenhuma mensagem nova sai ainda.

- **Protocolo 8** (`ir-proto`):
  - `BulkMessage::Folder(FolderMessage)`, no fim do enum, com 22 variantes próprias em
    `message/pasta.rs`: sessão, ciclo de vida, índice, trechos, envio, operações e desfecho;
  - `version::supports_folders` é o portão, e `CURRENT` foi a 8 com `MIN_SUPPORTED` em 6;
  - limites novos com asserções em compilação: 4 MiB por pedido de trecho, nome de pasta de 255 bytes,
    64 pastas no `Hello`, 100 mil entradas por pasta;
  - `is_safe_relative_path` saiu de `ManifestItem` para valer igual nos caminhos da pasta;
    `validate_folder_message` confere cada caminho, o nome, os tamanhos e a ordem das mudanças;
  - `changes_messages` reparte o índice em quadros.
- **Canal local do ajudante das pastas** (`ir-ipc/src/pastas.rs`):
  - `DoAjudanteDePastas` e `ParaOAjudanteDePastas`; o que vem do par vai com o tipo do protocolo;
  - o que a janela vê (`ResumoDePasta`, `ComandoDePasta`) tem vocabulário próprio;
  - endereço `\\.\pipe\inputremote-pastas` e `/run/inputremote/pastas.sock`, com sobrescrita
    `IR_PASTAS_ENDPOINT`.
- **Crate `ir-pasta`**, puro, na lista de pureza do `xtask`, com as duas regras que já estão decididas:
  - `conflito`: quem fica com o nome — pelo relógio corrigido pela diferença entre as máquinas, com
    empate da origem — e o nome da cópia, `nome (conflito NOTEBOOK 2026-10-02 14h30).ext`, que cabe
    em 255 bytes encurtando o nome original e nunca a etiqueta;
  - `ignorar`: os temporários de editor, os arquivos do sistema e a pasta de controle `.inputremote`.

## A versão que não pode ser partida em silêncio

O jeito do manifesto — encher o quadro e passar ao próximo — não basta para o índice. Duas entradas
podem ter a mesma versão — um conflito grava as duas de uma vez —, e a quebra pode
cair entre elas. Uma réplica que guardasse "apliquei até a versão da última entrada" e perdesse o
enlace ali pediria a continuação **depois** dessa versão, e a segunda entrada não chegaria nunca.

Agora o `up_to` de uma mensagem do meio é o número **anterior** à primeira versão da mensagem
seguinte. A réplica guarda o `up_to`, não a versão da última entrada, e uma versão partida é pedida
de novo inteira. Aplicar duas vezes a mesma entrada na mesma versão não muda nada. Teste:
`uma_versao_partida_entre_mensagens_e_pedida_de_novo_inteira`.

## O defeito que o teste pegou

`is_safe_component` não recusa `/`, porque recebe o caminho já partido nela. O nome da pasta oferecida
não vem partido: na primeira versão da validação, `"a/b"` passava, e a réplica criaria uma subpasta
com o nome que o par escolhesse. O teste `o_nome_da_pasta_e_um_componente_so` falhou, e a validação
do nome passou a recusar a barra.

## Arquivos

- `crates/ir-proto/src/message/pasta.rs`, `pasta/{validacao,paginas,testes}.rs`, `message/data.rs`,
  `message/mod.rs`, `version.rs`, `limits.rs`
- `crates/ir-proto/tests/vectors/pasta.rs`, `table.rs`, `main.rs`
- `crates/ir-ipc/src/pastas.rs`, `pastas/testes.rs`, `endereco.rs`, `lib.rs`
- `crates/ir-pasta/` (novo): `lib.rs`, `conflito.rs`, `conflito/testes.rs`, `ignorar.rs`
- `Cargo.toml`, `xtask/src/deps.rs`, `clippy.toml`
- `docs/adr/0015-pastas-compartilhadas.md`, `docs/00-indice.md`, `docs/02-arquitetura.md`,
  `docs/03-protocolo.md`, `PROGRESSO.md`

## Verificação

- **Vetores gravados da versão 8:** um por variante de `FolderMessage` e a saudação `hello_v8`.
  `every_folder_message_variant_is_recorded_once` confere, **nos bytes gravados**, que o terceiro byte
  de cada vetor da pasta cobre as 22 variantes uma vez só. Os vetores das versões 6 e 7 ficaram
  intactos.
- **Cabe no quadro**, codificando o pior caso, com sequência, confirmação e encarnação no máximo:
  - trecho e bloco de envio cheios, com pelo menos 64 bytes de folga;
  - envio e renomeação com caminhos de 1 024 bytes;
  - o `Hello` com 64 pastas;
  - 2 000 entradas de caminho máximo repartidas e remontadas iguais.
- **Portão:** `folders_go_only_to_a_peer_that_agreed_on_them` — fechado para a 7, também depois de
  negociar com um par da 7.
- **Canal local:** o número de cada variante no fio fixado por teste; um trecho cheio do par cabe numa
  mensagem do canal local.
- **`ir-pasta`:** o calendário nas viradas (antes de 1970, 29 de fevereiro de 2000 e de 2024, 2100 sem
  dia 29); a etiqueta; arquivo sem extensão e com ponto no começo; nome ocupado ganha número; o corte
  em 255 bytes sem partir caractere; o nome que o par diz de si não vira caminho; relógio adiantado
  não ganha conflito.
- `cargo xtask check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt` e
  `cargo test --workspace`: limpos, no Windows. A suíte toda tem 1 267 testes passando em 84 binários
  e 7 ignorados, os mesmos de antes, que dependem de hardware. Não rodei no Linux: a F0 não tem código
  de plataforma, e a CI cobre a matriz Ubuntu e Windows.

## Decisões

- **Nomes em inglês no fio, português no que a janela vê**, a regra de sempre: `FolderMessage`,
  `Entry` e `OpResult` em `ir-proto`; `ResumoDePasta` e `ComandoDePasta` em `ir-ipc`.
- **`Pedido` e `Aviso` do canal de controle não ganharam variante ainda.** Uma variante sem quem a
  atenda no serviço seria uma resposta "ainda não" escrita para ser apagada; entram na F1, com o
  repasse.
- **O ADR das dependências novas** (`notify`, seletor de pasta, *features* de Cloud Files) **fica para
  quando cada uma entrar.** A F0 não acrescenta nenhuma: `ir-pasta` depende só de `ir-proto`.
- **`ir-sincronia` e `ir-nuvem` não foram criados vazios.** Um crate sem código não prova fronteira
  nenhuma; as setas deles entram com eles.
- **O vocabulário do canal local pode mudar até a F1 ser lançada**: ninguém o fala ainda. Depois disso,
  só variante no fim.
