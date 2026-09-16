# O Bluetooth que não existia, e o portador que o serviço ignorava

**Data:** 2026-09-15

**Itens:** Etapa 7 — `ir-bt` (trait + backend Winsock + backend BlueZ) e política única de escolha
de portador. ADR-0009. Extração do `ir-transporte`.

## O que quem usa viu

> "estou com a mensagem: *bluetooth indisponível, usando a rede local*, porém tem o bluetooth e
> está disponível, inclusive está conectado o bluetooth dos 2 equipamentos."

## A mensagem estava certa

E é o que torna este caso interessante: não havia defeito na frase, havia ausência de produto
atrás dela.

A frase vem de `CarrierChoice::FellBackToNetwork`, que a política única do `ir-session` escolhe
quando `CarrierSet.rfcomm` é falso. E `rfcomm` nunca foi verdadeiro em máquina nenhuma, porque
**o crate `ir-bt` não existia** — a Etapa 7 estava inteira em aberto. O serviço dizia a verdade
sobre si mesmo; o que faltava era o que ele descrevia.

Os dois computadores estarem pareados no sistema operacional não muda isso. Pareamento de sistema
e conexão do produto são camadas diferentes ([ADR-0005](../adr/0005-bluetooth-rfcomm-winsock.md)):
o par estar pareado é pré-requisito, não é a conexão.

## O segundo defeito, que só apareceria depois

Antes de escrever o transporte, uma leitura do serviço mostrou algo que teria transformado o
Bluetooth em um sintoma pior que a ausência dele. Em `commands.rs`:

```rust
fn send_frame(&self, carrier: Carrier, frame: &Frame) {
    match ir_proto::codec::encode(frame, carrier) {
        Ok(bytes) => { let _ = self.net.send(NetCommand::SendFrame(bytes)); }
        //                          ^^^^^^^^ o portador chegava, e era descartado aqui
```

O portador vinha da sessão em cada comando e era ignorado. Com um transporte só, isso nunca
apareceu. Com dois, a tela diria "Bluetooth" e os bytes sairiam pela rede — e o limite de tamanho
conferido na codificação seria o do portador errado, já que o teto do RFCOMM (512 B) é menos da
metade do da rede (1 200 B).

Havia mais três do mesmo tipo: `Input::CarrierUp(Carrier::Udp)` fixo em três lugares,
`codec::decode(bytes, Carrier::Udp)` fixo na recepção, e — o mais direto — o motivo do portador
em `pedidos.rs` era literalmente a constante `MotivoDoPortador::RedeComoAlternativa`. Mesmo com o
rádio de pé, a tela continuaria dizendo a frase da reclamação.

## O que foi feito

### `ir-bt`: o transporte

Espelha a forma do `ir-net` — mesmos comandos, mesmos eventos, **mesmo `ir-crypto`**, sem uma
linha de criptografia própria. A camada que muda é a 0, e a consequência dela na 2.

| | UDP | RFCOMM |
|---|---|---|
| Enquadramento | uma mensagem por datagrama | prefixo de tamanho `u16` |
| Contador do Noise | viaja em claro no quadro | **contado nas duas pontas** |
| Quadro que não abre | ignorado | derruba o enlace |

O contador não viajar é o que [03, §3.1](../03-protocolo.md) manda para *stream*, e economiza
8 bytes em cada quadro num portador que tem 512 B. O preço é que as duas pontas precisam
concordar na contagem — e, como o meio não perde nem reordena, só divergem sob adulteração, que a
tag do Noise pega. Divergir em silêncio não é um resultado possível.

O quadro ruim derrubar o enlace é o **oposto** da regra do UDP, e de propósito: lá um datagrama
solto não pode virar negação de serviço; aqui um quadro que não abre significa contagem perdida, e
ignorá-lo deixaria o enlace vivo e mudo — nem funciona, nem cai.

### A fronteira que torna o rádio testável

Todo o protocolo trabalha contra um *trait* `Radio` e um canal de bytes (`AsyncRead + AsyncWrite`).
Os backends ficam abaixo. O efeito é que **o pareamento inteiro — código de seis dígitos, as duas
confirmações, o quadro que só passa depois delas — roda no CI em milissegundos**, com dois
endpoints ligados por um rádio de mentira. Foi a impossibilidade disso que deixou o Bluetooth do
v1 sem um único teste ([00, §6](../00-licoes-do-v1.md)).

### `ir-transporte`: a fronteira que faltava no serviço

O serviço passou a depender de um *trait* `Transporte` e de um `Fato` que **sempre diz por qual
portador veio**. Os dois adaptadores — um sobre o `ir-net`, outro sobre o `ir-bt` — são finos e
não decidem nada: a política de escolha continua única e no `ir-session`.

Um efeito colateral que valeu por si: `Endereco` lê `10.0.0.135:52525` e `AC:50:DE:47:EB:28` do
mesmo campo de texto, e as duas formas não se confundem (dois grupos contra seis). Com isso o
Bluetooth entrou **sem vocabulário novo na interface** — `IniciarPareamento { candidato: String }`
já servia.

A extração para crate próprio não foi estética: o `xtask` reprovou o `ir-daemon` em 2 684 linhas
de produção, teto 2 500. A fronteira estava comprovada, e extrair devolveu o crate a 2 418.

### ADR-0009: canal fixo, sem SDP

O [ADR-0005](../adr/0005-bluetooth-rfcomm-winsock.md) previa `ProfileManager1.RegisterProfile` no
Linux. Ao montar o crate, a resolução de dependências mostrou que o `bluer` com apenas o recurso
`rfcomm` não traz D-Bus nenhum — mas `RegisterProfile` **é** uma chamada de D-Bus, e adotá-la
traria libdbus para toda a cadeia de compilação do Linux, além de uma política de D-Bus no RPM.

Como as duas pontas são do mesmo produto, o SDP só estaria mapeando serviço para número de canal —
uma indireção que não leva informação. O produto atende no canal 23 fixo nas duas plataformas.
O que se perde está escrito no [ADR-0009](../adr/0009-canal-rfcomm-fixo-sem-sdp.md).

## Verificação

| O quê | Windows | Fedora 44 (container) |
|---|---:|---:|
| `ir-bt` | 63 testes | 67 testes |
| `ir-daemon` + `ir-transporte` | 42 testes | 42 testes |
| `clippy --all-targets` | sem avisos | sem avisos |

Os quatro testes a mais no Fedora são os do backend Linux, que só existem lá. O `xtask` aprova os
160 arquivos: tamanhos, setas de dependência e privacidade dos registros.

A compilação do Linux acontece dentro do Fedora 44 de verdade — nesta bancada Windows falta
compilador C cruzado, e o container é o mesmo que constrói o RPM. Ele compilou o `bluer` **sem
nenhum cabeçalho de BlueZ instalado**, que é a premissa do ADR-0009 provada por compilação, e não
só por resolução de dependências.

## O que o rádio mostrou

Primeira passagem pelo hardware, com os binários de release nas duas máquinas:

| Fato | Windows (`74:13:EA:A6:5A:99`) | Fedora 44 (`AC:50:DE:47:EB:28`) |
|---|---|---|
| Rádio encontrado | `BluetoothFindFirstRadio` achou | `hci0`, sem bloqueio de `rfkill` |
| Canal 23 | `bind` + `listen` funcionaram | `bind` + `listen` funcionaram |
| Registro publicado | **nenhum** (sem `WSASetService`) | **nenhum** (sem SDP, sem D-Bus) |

Os dois lados registraram `rádio Bluetooth aberto; é o portador preferido para teclado e mouse`.
Isso fecha a metade de **escutar** da hipótese do ADR-0009: vincular um canal fixo sem publicar
registro é aceito pelas duas pilhas, e o 23 está livre nas duas máquinas.

## O que ainda não foi provado

Nenhuma **conexão** entre os dois aconteceu ainda. O que falta, e é a PoC-2:

- que o Windows aceita uma conexão **de entrada** sem registro SDP publicado — é a outra metade
  da hipótese do ADR-0009, e a que o derruba se falhar (`WSASetService` volta, e só do lado do
  Windows, onde não custa dependência);
- o pareamento por rádio de ponta a ponta: código de seis dígitos, as duas confirmações, e um
  quadro atravessando;
- latência (mediana < 20 ms, p99 < 50 ms), reconexão < 5 s, MTU efetiva medida;
- as quatro combinações entre plataformas;
- que o socket abre **na sessão 0**, sem usuário logado. O teste acima rodou em primeiro plano,
  e não a partir do serviço — o item `[H]` continua aberto por isso.

O impedimento do momento é de bancada, não de código: o serviço instalado no Windows é do binário
antigo e segura `\\.\pipe\inputremote-control` e a porta UDP; esta sessão não tem privilégio para
pará-lo, e duas instâncias não convivem.

## Uma nota sobre o prazo de queda

O `link_timeout` é de 1 s e **global** — não há variante por portador
([`ir-session/src/config.rs`](../../crates/ir-session/src/config.rs)). Sobre RFCOMM, travamento
de rádio e *page timeout* passam de 1 s com facilidade, e o sintoma seria "conecta e cai" — o
mesmo dos logs 22 e 23, por causa diferente. Quem for ao rádio depois: olhe o prazo primeiro, e
não a época de sessão nem a retransmissão. A decisão sobre esse prazo é do usuário e segue
pendente desde o log 24; ela agora governa os dois portadores.

**Decisões:** o contador implícito sobre *stream*; o quadro ruim derrubando o enlace no RFCOMM e
não no UDP; canal fixo sem SDP (ADR-0009); a leitura do armazenamento do BlueZ em vez de D-Bus
para saber quem está pareado; e a extração do `ir-transporte` como crate, com a seta
`ir-daemon ──► ir-transporte ──► {ir-net, ir-bt}` substituindo as duas setas diretas.

**Arquivos:** `crates/ir-bt/` (novo, 11 arquivos), `crates/ir-transporte/` (novo, 4 arquivos),
`crates/ir-daemon/src/actor/enlace.rs` (novo), `crates/ir-daemon/src/{main,commands,config}.rs`,
`crates/ir-daemon/src/actor/{mod,partes,pedidos,pareamento,papel,bancada}.rs`,
`docs/adr/0009-canal-rfcomm-fixo-sem-sdp.md` (novo), `docs/adr/0005-...md`,
`docs/02-arquitetura.md`, `xtask/src/deps.rs`, `Cargo.toml`.
