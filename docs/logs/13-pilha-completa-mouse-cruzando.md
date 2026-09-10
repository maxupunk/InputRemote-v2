# A pilha completa: do pareamento cifrado à sessão de pé

**Data:** 2026-09-10

**Itens:** Etapas 3 (cripto), 4 (rede, parte de entrada), 5 (entrada no Windows, sessão
desbloqueada) e 1.3 (o binário do serviço). O que faltava para sair do modo demonstração e ter o
teclado e o mouse do Windows chegando ao Linux de verdade.

## A decisão que guiou tudo

O usuário escolheu **Noise + código de 6 dígitos desde o primeiro teste**, então a criptografia
veio antes do primeiro movimento de mouse, e não depois. O primeiro portador é a **rede UDP** — a
documentação trata os três portadores como intercambiáveis no protocolo, e UDP é o que dá para
testar entre duas máquinas sem depender de pareamento de rádio. Windows é o servidor (captura),
Linux é o cliente (injeta por `uinput`), que é exatamente o caso do relato.

## Os quatro crates novos

**`ir-crypto`** — identidade X25519 por máquina (com a pública derivada por `x25519-dalek`, para a
impressão digital exibida e a chave fixada serem sempre a mesma), `Noise_XX` para o primeiro
pareamento e `Noise_IK` com chave fixada para reconectar, o código SAS derivado do hash do
handshake, e o transporte cifrado com contador explícito e janela de repetição de 2048 bits. O
teste que importa: um homem no meio produz códigos diferentes nos dois lados.

**`ir-net`** — o endpoint UDP, uma tarefa dona do socket que fala com o serviço por dois canais. O
enquadramento de datagrama é puro e testado; o handshake Noise é conduzido sobre o socket; a
descoberta é mDNS. O pareamento exige a confirmação dos **dois** lados antes de qualquer quadro de
sessão. Dois endpoints em loopback fazem o pareamento inteiro e um quadro atravessa cifrado — a
prova de cripto + rede juntos, sem hardware.

**`ir-input`** — captura no Windows (`WH_MOUSE_LL` + `WH_KEYBOARD_LL` numa thread com laço de
mensagens, o gancho sem fazer trabalho) e injeção no Linux (`uinput`, dois dispositivos criados na
subida, ponteiro absoluto). O `unsafe` fica confinado aos módulos de backend. A captura foi
exercitada de verdade: 477 eventos com deltas corretos. A injeção do Windows moveu o cursor para a
coordenada exata injetada.

**`ir-daemon`** — o ator central de [02, §4](../02-arquitetura.md): o único dono da `Session`,
batendo-a a cada 5 ms para os prazos vencerem, convertendo captura e rede em `Input` e comandos em
efeitos. Configuração e identidade persistentes por máquina.

## A camada que amarra as duas

Vale registrar o desenho, porque ele não é óbvio: há **dois handshakes**, um dentro do outro. O
`ir-net` faz o handshake **de criptografia** (estabelece o canal cifrado). Só depois, o `ir-daemon`
diz à sessão `CarrierUp(Udp)`, e a sessão faz o seu próprio handshake **de aplicação** (`Hello` /
`HelloAck`, versão e capacidades) — e esses quadros viajam **dentro** do canal cifrado. A sessão
nunca vê a criptografia; a criptografia nunca vê a sessão. É a mesma disciplina de fronteira do
resto do projeto.

## Verificado de ponta a ponta

Dois daemons em loopback, pela pilha real (cripto + rede + sessão), sem simulação:

- **Pareamento:** os dois mostram o mesmo código de 6 dígitos, confirmam com `s`, e logam
  `sessão estabelecida ... carrier=udp` nos dois lados.
- **Persistência:** a chave do par é gravada no `config.toml`.
- **Reconexão:** na subida seguinte, sem código, os dois estabelecem imediatamente por `Noise_IK`
  com a chave fixada.

O que **não** pôde ser verificado numa máquina só: a travessia física. A captura ignora eventos de
mouse injetados (para não recapturar a própria injeção, corretamente), então um `SetCursorPos` de
script não dispara a captura — só um mouse físico dispara. A travessia em si, portanto, se verifica
movendo um mouse de verdade até a borda, o que é a ação do usuário nas duas máquinas.

Cada elo isolado, porém, está provado: captura (477 eventos reais), travessia (testes de unidade
de `ir-session`), rede (pareamento e quadro de ponta a ponta), injeção (cursor movido). O
`ir-input` e o `ir-daemon` compilam também para Linux, verificados no container do Fedora.

## Como usar

[USAR.md](../../USAR.md) traz o passo a passo: configurar papel e endereço, subir o Linux primeiro,
subir o Windows, comparar o código, e atravessar. É o nível **N1** — as duas máquinas
desbloqueadas.

## O que fica para depois

- **Tela de bloqueio (N2/N3):** o agente no desktop seguro e a assinatura de código.
- **Bluetooth:** o portador principal do projeto; hoje só a rede UDP.
- **Interface ligada ao serviço:** a janela já existe, mas fala com o serviço simulado; falta o
  transporte de `ir-ipc` de verdade.
- **Rechaveamento automático, TCP de dados, clipboard e arquivos.**

## Uma escolha honesta sobre o backend Linux

O `uinput` foi escrito sem poder ser compilado na máquina de desenvolvimento (Windows), e
verificado depois no container do Fedora. Ele usa o `evdev` 0.12 com eixo absoluto `0..=65535`,
seguindo o desenho de [06, §2](../06-linux.md). A injeção em si só pode ser exercitada numa máquina
Linux com acesso a `/dev/uinput` — é o próximo ponto de verificação em hardware.
