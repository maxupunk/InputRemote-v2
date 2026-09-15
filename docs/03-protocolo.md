# 03 — Protocolo

## 1. Camadas

O protocolo é o mesmo nos três portadores. Só a camada 0 muda.

```text
 L4  Mensagens        Hello, KeyDown, PointerMotion, ClipOffer, FileBlock, ...
 L3  Canais           Controle | Entrada confiável | Ponteiro | Clipboard | Dados
 L2  Enquadramento    prefixo de tamanho (stream) | 1 mensagem por datagrama (UDP)
 L1  Criptografia     Noise — IDÊNTICA nos três portadores
 L0  Portador         RFCOMM (stream) | UDP (datagrama) | TCP (stream)
```

Uma camada L1 só, para os três portadores, é a principal simplificação em relação ao v1,
que tinha SPAKE2 + TLS/QUIC + enquadramento próprio de RFCOMM convivendo.
Ver [ADR-0003](adr/0003-noise-em-vez-de-quic.md).

## 2. Portadores

| Portador | Uso | Garantia do meio | Enquadramento |
|---|---|---|---|
| Bluetooth RFCOMM | entrada, controle, texto curto | stream confiável e ordenado | `u16` de tamanho + corpo |
| UDP | entrada, controle, texto curto | nenhuma | uma mensagem por datagrama |
| TCP | clipboard grande, imagens, arquivos | stream confiável e ordenado | `u32` de tamanho + corpo |

Regras rígidas:

- entrada **NUNCA** viaja no TCP;
- arquivos e imagens **NUNCA** viajam no RFCOMM nem no UDP;
- os portadores de entrada (RFCOMM e UDP) são mutuamente exclusivos numa sessão: não há
  duplicação de eventos nem "o que chegar primeiro vence".

Tamanhos máximos de quadro:

| Portador | Máximo de texto claro | Motivo |
|---|---:|---|
| UDP | 1 200 B | fica abaixo da MTU típica sem fragmentar IP |
| RFCOMM | negociado, teto de 512 B | MTU de RFCOMM varia entre pilhas; 512 B é seguro em todas |
| TCP | 64 KiB | blocos de arquivo |

## 3. Criptografia (L1)

Duas situações, dois padrões Noise, ambos com X25519, ChaCha20-Poly1305 e BLAKE2s.
Nenhum PAKE, nenhum TLS, nenhum certificado.

**Primeiro pareamento — `Noise_XX_25519_ChaChaPoly_BLAKE2s`.**
Os dois lados ainda não se conhecem. O handshake roda sem autenticação prévia e, ao
final, cada lado deriva do hash do handshake um código de seis dígitos. O usuário compara
os dois códigos, olhando as duas telas, e confirma nos dois lados; só então cada lado
grava a chave estática pública do outro. Detalhes em [04](04-seguranca.md).

**Toda sessão posterior — `Noise_IK_25519_ChaChaPoly_BLAKE2s`.**
O iniciador já conhece a chave estática do respondedor. Handshake de 1,5 ida-e-volta,
e a conexão é rejeitada se a chave estática apresentada não for **exatamente** a que
está fixada. Não existe "confiar na primeira vez" depois do pareamento.

### 3.1. Quadro cifrado

Em stream (RFCOMM, TCP):

```text
┌────────────┬──────────────────────────────┬──────────┐
│ tamanho    │ texto cifrado                │ tag 16 B │
│ u16 ou u32 │                              │          │
└────────────┴──────────────────────────────┴──────────┘
```

Em datagrama (UDP), o contador é explícito porque a ordem não é garantida:

```text
┌───────────────┬──────────────────────────────┬──────────┐
│ contador u64  │ texto cifrado                │ tag 16 B │
└───────────────┴──────────────────────────────┴──────────┘
```

O contador é o nonce Noise. O receptor mantém uma **janela deslizante de 2 048 bits**:
contador já visto ou anterior à janela é descartado sem processar. Isso impede reenvio
de um `KeyDown` capturado do ar — em um produto que digita senhas, repetição não é
detalhe acadêmico.

Rechaveamento a cada 2^20 mensagens ou 10 minutos, o que vier primeiro.

## 4. Canais (L3)

O primeiro byte do texto claro identifica o canal.

| Id | Canal | Direção | Garantia da aplicação | Portadores |
|---:|---|---|---|---|
| 0 | Controle | bidirecional | confiável, ordenado | RFCOMM, UDP, TCP |
| 1 | Entrada confiável | servidor→cliente | confiável, ordenado | RFCOMM, UDP |
| 2 | Ponteiro | servidor→cliente | o mais recente vence | RFCOMM, UDP |
| 3 | Retorno | cliente→servidor | confiável, ordenado | RFCOMM, UDP |
| 4 | Clipboard de texto | bidirecional | confiável, fragmentado | RFCOMM, UDP, TCP |
| 5 | Dados | bidirecional | confiável, ordenado | somente TCP |

Sobre RFCOMM e TCP, o portador já é confiável e ordenado: os canais 0, 1, 3, 4 e 5 não
acrescentam nada além do número de sequência para diagnóstico.

Sobre UDP, o portador não garante nada, então:

### 4.1. Confiabilidade dos canais 0, 1, 3 e 4 sobre UDP

Um mecanismo pequeno e explícito, não uma reimplementação de TCP:

- cada mensagem carrega `seq: u32` do canal;
- toda mensagem de volta carrega `ack: u32` (maior sequência contígua recebida) e
  `ack_bits: u32` (as 32 anteriores, em bitmap);
- o emissor guarda as não confirmadas numa janela de no máximo 64 mensagens;
- retransmissão após `RTO = max(20 ms, 2 × srtt)`, **dobrando a cada reenvio** até o teto de
  um quarto do prazo de queda — no piso, 20, 40, 80, 160 e depois 250 ms entre um e outro — e
  continuando até o prazo, para que a mensagem perdida num pico ainda tenha como chegar;
- o enlace é declarado caído quando uma mensagem fica **mais de 1 s sem confirmação, contado
  do primeiro envio** — o mesmo prazo de queda da sessão. Não se prossegue com lacuna: um
  `KeyUp` perdido é uma tecla presa, e cair é melhor que travar.

Desistir por tempo, e não por contagem de tentativas, é o que separa um pico de latência de um
par que sumiu. Com prazo fixo de 20 ms e cinco tentativas, a sessão desistia em ~100 ms, e um
Wi-Fi com economia de energia — que segura quadros por mais de 100 ms de vez em quando —
derrubava a sessão a cada pico. Esperar não cria lacuna: a mensagem continua na fila, na ordem,
e só chega mais tarde.

Quando não há tráfego de volta, o receptor manda um `Ack` puro a cada 20 ms enquanto
houver algo pendente.

### 4.2. Canal 2, ponteiro

Sem confirmação e sem retransmissão. Carrega `seq: u32`; o receptor descarta qualquer
mensagem com sequência anterior à última aceita. Perder amostras de movimento é
invisível; atrasá-las não é.

### 4.3. Encarnações de sessão

Cada aperto de mão começa uma **encarnação** nova da sessão, e todo quadro carrega, **no fim**,
a `epoch: u32` da encarnação de quem o enviou. O primeiro byte continua sendo o canal.

A época não tem ordem: só precisa diferir da encarnação anterior. Cada ponta a deriva de uma
semente sorteada pelo serviço a cada sessão criada; o núcleo da sessão não sorteia nada.

Quem recebe decide **antes** de qualquer outra coisa — prova de vida, confirmação, ordenação:

| O quadro é | E a época é | Então |
|---|---|---|
| qualquer um | a da sessão corrente do par | segue o caminho normal |
| `Hello` ou `HelloAck` | nova | o par começou outra sessão; se havia uma de pé, ela é encerrada soltando tudo, e esta ponta recomeça junto |
| `Hello` ou `HelloAck` | uma das 4 últimas aposentadas | eco atrasado de uma sessão que acabou; descartado |
| qualquer outro | diferente da corrente | resto de outra sessão; descartado |

Sem isto, o lado que acabara de zerar se ancorava num quadro velho do par, e o `Hello` novo,
de número 1, parecia mais velho que a âncora e era descartado calado: os dois lados
reiniciavam a sessão a cada ~200 ms, para sempre. Pior, um `KeyDown` velho guardado na fila de
reordenação podia ser entregue na sessão nova como tecla digitada agora.

## 5. Modelo de teclado

A chave física viaja como **HID Usage ID (Usage Page 0x07)**. Não viaja caractere, não
viaja código virtual do Windows, não viaja *keysym*.

| Origem | Conversão |
|---|---|
| Windows | scancode de `WM_INPUT` / gancho → HID Usage |
| Linux | `KEY_*` do evdev → HID Usage |

O caractere é decidido pelo layout do computador **controlado**. É o comportamento certo
para o requisito de tela de login: quem digita a senha precisa que o teclado se comporte
como o teclado daquela máquina.

Modificadores viajam de duas formas simultâneas:

- como eventos (`KeyDown`/`KeyUp` do próprio modificador), e
- como **estado**, em toda mensagem de entrada (um `u8` de bitmap: Ctrl, Shift, Alt, Meta
  esquerdo e direito).

Quando o estado recebido diverge do estado aplicado, o cliente corrige antes de processar
o evento. Esta redundância custa 1 byte por mensagem e elimina a categoria inteira de
bugs de "Ctrl ficou preso".

## 6. Catálogo de mensagens

Nomes definitivos vivem em `ir-proto`. Este é o contrato.

### Canal 0 — Controle

| Mensagem | Conteúdo |
|---|---|
| `Hello` | versão do protocolo, id da máquina, nome, capacidades |
| `HelloAck` | versão acordada, capacidades do par |
| `ScreenLayout` | monitores: id, retângulo, escala, monitor primário |
| `EdgeConfig` | a borda do servidor que dá para o cliente. Só o servidor envia, ao estabelecer e a cada troca; o cliente usa a oposta, e o servidor ignora um que receba. Trocar a borda não refaz a sessão (log 24) |
| `EnterScreen` | o controle passou para o par: posição de entrada, borda, estado de modificadores |
| `LeaveScreen` | o controle voltou: posição de saída, borda |
| `StateSnapshot` | conjunto completo de teclas e botões pressionados (§7) |
| `Ping` / `Pong` | carimbo monotônico, para latência e detecção de queda |
| `Bye` | motivo legível de encerramento |
| `Error` | código, contexto, se é fatal |

### Canal 1 — Entrada confiável

`KeyDown{usage, mods}`, `KeyUp{usage, mods}`, `ButtonDown{button, mods}`,
`ButtonUp{button, mods}`, `Wheel{dx, dy, mods}`, `ReleaseAll`.

### Canal 2 — Ponteiro

`PointerMotion{dx, dy, mods}` — relativo, o caso comum.
`PointerPosition{monitor, x, y, mods}` — absoluto, para correção e entrada de borda.

### Canal 3 — Retorno

`EdgeReached{borda, posição}`, `EmergencyRelease`, `ClipboardChanged{tipo, tamanho}`.

### Canal 4 — Clipboard de texto

`ClipOffer{tipo, tamanho, hash}`, `ClipRequest{id}`, `ClipChunk{id, índice, dados}`,
`ClipDone{id}`. Teto de 256 KiB fora do TCP; acima disso, só pelo canal 5.

### Canal 5 — Dados (só TCP)

`Manifest{itens, bytes totais}`, `FileStart{id, caminho relativo, tamanho, modo}`,
`FileBlock{id, deslocamento, dados}`, `FileEnd{id, blake3}`, `Verified{id, ok}`,
`Progress{id, bytes}`, `Cancel{id, motivo}`.

## 7. `StateSnapshot` — a rede de segurança

O servidor envia a cada 250 ms enquanto o controle estiver no par, e sempre depois de:
troca de portador, reconexão, subida de agente novo, troca de desktop no Windows,
retorno de suspensão.

Conteúdo: bitmap de todas as teclas pressionadas (por HID Usage), bitmap de botões,
bitmap de modificadores e a posição do ponteiro.

O cliente **reconcilia**: solta o que está pressionado localmente e não está no snapshot,
pressiona o que está no snapshot e não está pressionado. A operação é idempotente.

Este é o mecanismo que garante a meta "zero teclas presas em 10.000 travessias" de
[01, §6](01-visao-e-escopo.md), independentemente da causa da perda.

## 8. Versionamento

`Hello` carrega `protocol_version: u16`. Vale a menor versão entre as duas pontas. Se a
diferença for maior que uma versão maior, a sessão é recusada com mensagem explícita.

Regra deliberadamente estrita: **mensagem desconhecida em canal confiável derruba o
enlace**; campo desconhecido não é ignorado. Um KVM que age sob ambiguidade digita a
coisa errada na máquina do outro lado. Compatibilidade se resolve na negociação de
versão, não na tolerância do decodificador.

## 9. Serialização

`postcard` sobre `serde`. Motivos: formato binário compacto, sem alocação na decodificação,
e implementação em Rust puro e auditável.

Uma consequência que precisa estar dita, porque é contraintuitiva: **`postcard` não é
autodescritivo**. Os campos são posicionais, não nomeados. Portanto não existe "campo
desconhecido" para ignorar nem para recusar — `#[serde(deny_unknown_fields)]` não teria
efeito aqui — e qualquer mudança na ordem ou no tipo dos campos é quebra de compatibilidade
silenciosa: o decodificador do outro lado lê bytes válidos e produz um valor errado.

Por isso a compatibilidade é resolvida **inteiramente** pela negociação de versão da §8, e
nunca pela tolerância do decodificador. Regras que sustentam isso:

- alterar um tipo do `ir-proto` **DEVE** incrementar `protocol_version`;
- um teste com vetores gravados (bytes de referência por versão) falha se a codificação de
  uma mensagem mudar sem o incremento — é a rede de proteção contra a quebra silenciosa;
- toda mensagem carrega um discriminante de tipo explícito, nunca implícito pela posição.

Todo tipo do `ir-proto` **DEVE** ter:

- teste de ida e volta (`encode → decode` preserva o valor);
- teste de tamanho máximo (nenhuma mensagem de entrada passa de 64 B em texto claro);
- fuzzing do decodificador contra entrada arbitrária, no CI ([10](10-testes-e-validacao.md)).

O decodificador é o código que recebe bytes de um rádio aberto, dentro de um processo
SYSTEM. É o ponto mais sensível do produto inteiro.

## 10. Descoberta

`_inputremote._udp.local` por mDNS, anunciando: id da máquina, nome legível, versão do
protocolo, porta e se já existe pareamento com quem pergunta (por um identificador
derivado, não pela chave). O anúncio **NÃO DEVE** conter nome de usuário, chave, código
de pareamento nem conteúdo de clipboard.

Endereço e porta manuais sempre disponíveis, para redes que isolam clientes entre si.
Porta padrão 52525, UDP e TCP.
