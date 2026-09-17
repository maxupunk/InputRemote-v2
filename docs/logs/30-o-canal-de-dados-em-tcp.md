# O canal de dados em TCP, e o teto que o Noise nunca teria permitido

**Data:** 2026-09-17

**Itens:** Etapa 2.1 — ida e volta de toda variante de `ClipboardMessage` e `BulkMessage`.
Etapa 4 — TCP de dados, em andamento. Etapa 8 — o transporte do canal 5.

**O que foi feito:** o portador do canal 5, que até aqui existia só como política. Um defeito de
aritmética apareceu antes da primeira linha de transporte, e dois desenhos previstos nos
documentos se mostraram errados quando encostados no código.

## O número impossível

`MAX_TCP_PLAINTEXT` era `64 * 1024`. Uma mensagem de transporte Noise tem no máximo 65 535 bytes
**contando a etiqueta Poly1305 de 16**, logo o texto claro para em 65 519.

O valor antigo estava dezessete bytes acima do possível. Não era margem apertada; era impossível:
`snow` recusa `payload + 16 > 65535` com `Error::Input`, e o primeiro bloco cheio de arquivo nunca
teria sido cifrado. O número sobreviveu dois meses porque foi escrito na especificação
([03, §2](../03-protocolo.md)), copiado para o código, e nunca exercitado — nada usava TCP.

Está corrigido para 65 519, com asserção de tempo de compilação amarrando o número ao teto do
Noise, e há agora um `MAX_FILE_BLOCK` de 60 KiB separado para o conteúdo de bloco. A folga do
cabeçalho é conferida **codificando o pior caso**, não pela aritmética do `postcard`:

```text
a_full_file_block_fits_a_tcp_frame          bloco de 60 KiB + cabeçalho cabe, com ≥ 64 B de folga
a_frame_past_the_noise_ceiling_is_refused   acima de 65 519 o codec recusa, em vez de o par cair
the_plaintext_ceiling_is_where_we_think     medido contra o `snow`: 65 519 passa, 65 520 não
```

O último roda no `ir-crypto` e é o que transforma o número de leitura de especificação em fato
medido.

## Duas previsões dos documentos que não se sustentaram

**A primeira.** O `ir-transporte` prometia, no próprio doc de topo, que *"acrescentar o TCP de
arquivos, depois, é acrescentar um terceiro adaptador — e nada no ator muda"*. Não é. O
`trait Transporte` foi desenhado a partir de dois portadores que levam quadros de entrada e
começam com pareamento; o canal de dados não pareia, `Endereco::Rede` responde `Carrier::Udp`
incondicionalmente, e `enum Fato` não é `#[non_exhaustive]`, então as variantes de progresso
quebrariam todos os `match`. Pior: colocariam evento de arquivo no canal que o serviço consome no
compasso de 5 ms da entrada.

Decisão registrada em [ADR-0010](../adr/0010-canal-de-dados-em-tcp-proprio.md): interface própria,
fora da abstração de portador.

**A segunda.** A ideia natural seria um enlace único com `select!` entre enviar e receber. Está
errada, e a razão é sutil: `Frames::recv` lê do socket para um buffer e **depois** entrega os bytes
ao desenquadrador. Se o futuro for descartado entre as duas etapas, o que o socket já entregou
desaparece — o TCP considera aqueles bytes entregues e ninguém os pede de novo. O sintoma seria um
bloco faltando no meio de gigabytes, sem erro em lugar nenhum.

A saída foi dividir o socket em duas metades e remover a categoria do problema, em vez de tentar
acertar o cancelamento.

## O que a divisão exigiu, e o que ela custou

Dividir o socket obriga a dividir o transporte cifrado, e aí veio a parte boa: o estado de
transporte do Noise na forma *stateless* recebe `&self` nas duas direções — `write_message` e
`read_message` — porque o nonce vem de fora. As chaves não mudam a cada mensagem.

Logo o único estado mutável são duas coisas que não se cruzam: o contador de envio, que só quem
cifra toca, e a janela de repetição, que só quem decifra toca. `Transport::split` reconhece uma
separação que já existia, e o custo é um `Arc`. A alternativa era um `Mutex` no caminho de cada
bloco de arquivo, protegendo um estado que nunca é escrito.

A API pública do `Transport` não mudou: UDP e Bluetooth não sabem que isso aconteceu.

## A regra da colisão

As duas máquinas escutam na 52525 e as duas podem ter o endereço da outra — é a configuração da
bancada de hoje. Sem regra, um religar de rede produz duas conexões e a transferência depende de
qual o motor pegou primeiro.

**Sobrevive o enlace cujo iniciador tem a chave pública maior**, em ordem de bytes. Nenhuma
mensagem trocada: as duas chaves já são conhecidas dos dois lados desde o pareamento. A regra é
consultada só na colisão — com uma conexão só ela fica de pé, senão o lado de chave menor
derrubaria o único enlace existente quando ele é o único que sabe o endereço do outro.

## Arquivos

- `crates/ir-proto/src/limits.rs` — o teto corrigido, `MAX_FILE_BLOCK`, duas asserções novas
- `crates/ir-proto/src/codec.rs` — três testes do bloco cheio e do teto
- `crates/ir-proto/tests/vectors/dados.rs` (novo) — 14 vetores gravados dos canais 4 e 5
- `crates/ir-proto/tests/vectors/main.rs` — portador por canal; cobertura vinda de `ChannelId::ALL`
- `crates/ir-crypto/src/transport.rs` — `Sealer`, `Opener`, `Transport::split`
- `crates/ir-net/src/bulk/` (novo) — `wire`, `stream`, `link`, `handshake`, e a regra de colisão
- `crates/ir-net/src/error.rs` — `TooLarge` e `Closed`
- `docs/adr/0010-canal-de-dados-em-tcp-proprio.md` (novo), `docs/03-protocolo.md`,
  `docs/00-indice.md` (que também não listava o ADR-0009)

**Verificação:** `cargo test --workspace` verde; `ir-net` de 15 para 38 testes, `ir-crypto` 30,
`ir-proto` 149 + 7 de vetores. `cargo clippy --workspace --all-targets` silencioso, exit 0.
`cargo xtask check-limits`: 168 arquivos dentro das regras.

**Decisões:** [ADR-0010](../adr/0010-canal-de-dados-em-tcp-proprio.md). Mais duas menores: o
prefixo continua `u32` embora o corpo máximo corrigido caiba num `u16`, porque trocar formato de
fio para economizar dois bytes a cada 64 KiB é churn sem ganho; e `TCP_NODELAY` fica ligado, que
não muda nada nos blocos de 60 KiB mas evita o algoritmo de Nagle retendo um `Accept` de doze
bytes.

**O que ainda não foi provado:** que 5 GB atravessam sem passar de 10% de degradação da entrada —
critério de saída da Etapa 8, que exige medição na bancada. E que a regra da colisão resolve o caso
real com as duas pontas discando ao mesmo tempo depois de um religar de rede. Nada deste log
atravessou uma placa de rede ainda: são 38 testes sobre `tokio::io::duplex`.
