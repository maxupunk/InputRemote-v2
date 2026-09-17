# ADR-0010 — O canal de dados em TCP próprio, fora da abstração de portador

**Status:** aceito · **Data:** 2026-09-17 · **Completa:** [ADR-0003](0003-noise-em-vez-de-quic.md)

O produto tem dois transportes de entrada (`ir-net` por UDP, `ir-bt` por RFCOMM) atrás de uma
porta só, o `ir-transporte`, cujo doc de topo previa: *"acrescentar o TCP de arquivos, depois, é
acrescentar um terceiro adaptador — e nada no ator muda."*

Ao chegar a hora de acrescentá-lo, a previsão se mostrou errada. Este ADR registra o que foi
encontrado e o que se decidiu no lugar.

## O fato que apareceu depois

O `trait Transporte` foi desenhado a partir de dois portadores que fazem a mesma coisa: levam
quadros de sessão de teclado e mouse, um de cada vez, e começam com um pareamento. Ele tem cinco
métodos:

```rust
fn portador(&self) -> Carrier;
fn conectar(&self, alvo: Endereco, chave: Option<PublicKey>);
fn enviar(&self, bytes: Vec<u8>);
fn confirmar_pareamento(&self, conferiu: bool);
fn desconectar(&self);
```

Três atritos, nenhum deles cosmético:

1. **`confirmar_pareamento` não tem significado no TCP.** O canal de dados usa a identidade que
   *já* foi fixada pela sessão ([01, §3.3](../01-visao-e-escopo.md): "a mesma identidade da
   sessão"). Ele nunca pareia, nunca mostra código de seis dígitos, nunca é o primeiro encontro.
   Implementá-lo seria escrever um método que não pode ser chamado.
2. **`Endereco::Rede(SocketAddr)` responde `Carrier::Udp`, incondicionalmente.** Um `ip:porta` não
   consegue dizer "TCP". Como o serviço roteia pelo endereço, um terceiro portador exigiria ou uma
   variante nova de endereço para o mesmo tipo de endereço, ou trocar `Endereco` por um par
   `(endereço, portador)` — mexendo no caminho de entrada para servir o de dados.
3. **`enum Fato` não é `#[non_exhaustive]`.** Uma transferência precisa contar progresso, término
   e cancelamento; acrescentar essas variantes ao `Fato` quebraria todos os `match` dos
   consumidores, e pior: colocaria evento de arquivo no mesmo canal que o serviço consome no
   compasso de 5 ms da entrada.

O terceiro atrito é o que decide. **O canal de dados e o de entrada têm requisitos opostos**, e
isso já está escrito no produto: o `ir-proto` diz que ali "integridade vale mais que latência, o
oposto exato dos canais de entrada". Um bloco de 60 KiB atravessando a fila que existe para
entregar um `KeyUp` em milissegundos é o defeito que o critério de saída da Etapa 8 proíbe:
*"transferência de 5 GB sem degradar a latência da entrada além de 10%"*.

## A decisão

O canal de dados é **uma conexão TCP própria, com interface própria**, e não um terceiro
`Transporte`.

- **Transporte:** `ir-net::dados` — enquadramento `u32` + corpo ([03, §2](../03-protocolo.md)),
  Noise `IK` com a chave já fixada, nunca `XX`.
- **Porta de entrada única preservada:** o serviço continua sem falar com `ir-net` direto. O
  `ir-transporte` ganha uma **segunda** interface, `CanalDeDados`, ao lado do `Transporte` — a
  mesma porta, duas fechaduras, porque são duas necessidades.
- **Fora do compasso da sessão:** a transferência vive na sua própria tarefa. A sessão de entrada
  aprende dela **um bit** — se há canal de dados —, que é o que alimenta
  `Capabilities::bulk_transfer` e `clipboard::route`. Nada mais do canal 5 passa pelo
  `Session::step`.
- **Quem disca é determinístico:** as duas máquinas escutam na porta 52525; **disca a que tem a
  chave pública maior**, em ordem de bytes. A outra só escuta. A regra não custa mensagem nenhuma,
  as duas pontas já conhecem as duas chaves, e ela elimina a conexão cruzada — hoje as duas pontas
  da bancada têm `peer_addr` uma para a outra, então as duas discariam.

## O erro de aritmética que isto revelou

`MAX_TCP_PLAINTEXT` era `64 * 1024`. Uma mensagem de transporte Noise tem no máximo 65 535 bytes
**contando a etiqueta Poly1305 de 16**, logo o texto claro para em 65 519. O valor antigo estava
dezessete bytes acima do possível, e `snow` recusa `payload + 16 > 65535` com `Error::Input`.

Não era margem apertada: era impossível. O primeiro bloco cheio de arquivo nunca teria sido
cifrado. O número sobreviveu porque nada usava TCP — foi escrito na especificação, copiado para o
código, e nunca exercitado.

Corrigido para 65 519, com asserção de tempo de compilação que amarra o número ao teto do Noise, e
com um `MAX_FILE_BLOCK` de 60 KiB separado para o conteúdo de bloco, que desconta o cabeçalho da
mensagem. A folga é conferida por teste que codifica o pior caso, não pela aritmética do
`postcard`. [03, §2](../03-protocolo.md) foi corrigido junto.

## Por quê assim, e não das outras formas

**Por que não um `Transporte` com métodos que não fazem nada?** Porque um trait cujos
implementadores precisam de `unimplemented!()` deixou de ser uma abstração comum e passou a ser
duas abstrações escritas uma sobre a outra. O produto proíbe `unimplemented` por lint, e a razão
do lint é exatamente esta.

**Por que não reaproveitar o `EnlaceSeguro<C: Canal>` do `ir-bt`, que já é genérico em stream?**
Porque `ir-net` e `ir-bt` são crates de plataforma irmãos e [02, §2](../02-arquitetura.md) proíbe
um depender do outro. O que se repete entre os dois é pequeno — prefixo de tamanho e um contador
implícito — e a duplicação honesta custa menos que furar a regra de dependência ou criar um crate
só para hospedar cem linhas.

**Por que contador implícito, como no RFCOMM, e não explícito como no UDP?** Porque TCP é stream
ordenado e confiável: os dois lados contam os quadros e chegam ao mesmo número. Põr o contador no
fio gastaria 8 bytes por quadro para transportar uma informação que o receptor já tem. A
consequência é a mesma do RFCOMM: um quadro que não abre significa contadores dessincronizados, e
o enlace **cai** em vez de ignorar o quadro.

**Por que manter a conexão de pé em vez de discar sob demanda?** Porque [01, §5](../01-visao-e-escopo.md)
exige que o estado corrente seja observável — "arquivos indisponíveis" precisa ser verdade medida,
não suposição a partir de haver um endereço de rede. E porque um Ctrl+C não deve esperar um
*handshake*.

## O que se perde, e é aceito

- **Uma conexão TCP ociosa por par.** É o custo de poder dizer a verdade sobre disponibilidade.
- **Duas sessões Noise com a mesma identidade.** Não enfraquece nada — cada uma tem suas próprias
  chaves de transporte e sua própria janela de repetição —, mas são dois estados a manter.
- **Duas interfaces no `ir-transporte` em vez de uma.** Mais superfície; em troca, nenhum método
  vazio e nenhum evento de arquivo no caminho da entrada.

## O que ainda não foi provado

- Que 5 GB atravessam sem passar de 10% de degradação da entrada. É o critério de saída da Etapa 8
  e exige medição na bancada, não argumento.
- Que a regra da chave maior resolve a conexão cruzada nas duas plataformas, com as duas pontas
  discando ao mesmo tempo depois de um religar de rede.
