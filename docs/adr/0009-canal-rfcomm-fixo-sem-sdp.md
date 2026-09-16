# ADR-0009 — Canal RFCOMM fixo, sem SDP

**Status:** aceito · **Data:** 2026-09-15 · **Altera:** [ADR-0005](0005-bluetooth-rfcomm-winsock.md), a parte de SDP da Decisão A

O ADR-0005 decidiu o transporte Bluetooth e, junto, como cada lado publicaria e encontraria o
serviço: `WSASetService` e `WSALookupServiceBegin/Next` no Windows, e
`org.bluez.ProfileManager1.RegisterProfile` pelo D-Bus do sistema no Linux. O transporte não
muda. A publicação de serviço, sim.

## O fato que apareceu depois

Ao montar o `ir-bt`, a resolução de dependências do alvo Linux foi medida:

```text
bluer 0.17.4, default-features = false, features = ["rfcomm"]
  └── futures, hex, libc, log, macaddr, nix, num-derive, num-traits, strum
```

Nenhuma ligação com D-Bus. O recurso `rfcomm` do `bluer` é Rust puro sobre sockets
`AF_BLUETOOTH` do kernel — ele não fala com o `bluetoothd`.

Só que `ProfileManager1.RegisterProfile` **é** uma chamada de D-Bus. Adotá-lo traria a
biblioteca C do D-Bus de volta para a cadeia de compilação do Linux inteira, e o container de
Fedora 44 que constrói o RPM não tem esse cabeçalho instalado. Junto viria uma política de D-Bus
para empacotar e manter.

## A decisão

O produto **atende num canal RFCOMM fixo**, o 23, nas duas plataformas, e **conecta nesse mesmo
canal**. Não publica registro SDP e não faz consulta SDP.

O número está na faixa válida (1–30) e fora dos canais que os perfis comuns tomam primeiro.

## Por quê

O SDP, aqui, tem um trabalho só: dizer em qual número de canal o serviço atende. As duas pontas
são do mesmo produto e podem simplesmente concordar com uma constante. Isso não é uma troca de
funcionalidade por simplicidade — é remover uma indireção que não estava levando informação
nenhuma.

O custo dos dois caminhos não se compara: uma constante num arquivo, contra uma dependência
nativa em C na compilação de todo o Linux mais um arquivo de política no pacote.

**Descobrir o serviço é diferente de descobrir a máquina.** O que a tela "Procurar" precisa
listar é quais computadores estão pareados no sistema, e isso não vem de SDP: no Windows vem das
APIs de Bluetooth do Win32, e no Linux do próprio armazenamento do BlueZ. Essa parte continua de
pé e não depende desta decisão.

## O que se perde, e é aceito

- **Uma ferramenta de Bluetooth de terceiros não verá um registro de serviço do InputRemote.**
  Perde-se um caminho de diagnóstico. Mitigado pelo diagnóstico próprio, que já é obrigado a
  distinguir "não pareado no sistema" de "pareado, mas o serviço não responde"
  ([ADR-0005](0005-bluetooth-rfcomm-winsock.md), Consequências) — e que é a pergunta que o
  usuário realmente faz.
- **Se outro aplicativo já ocupar o canal 23 numa das máquinas, o vínculo falha.** O produto
  detecta e diz qual é o problema; ele não sai procurando outro canal em silêncio, porque aí as
  duas pontas deixariam de concordar e o sintoma viraria "às vezes conecta".
- **Interoperar com um par que não seja o InputRemote está fora de escopo.** O protocolo é nosso
  nas duas pontas.

## Reversível sem quebrar nada

Se o SDP entrar um dia, ele entra **sem trocar o número**: o registro anunciaria o canal 23, e
quem não encontrasse registro nenhum cairia nele do mesmo jeito. As duas pontas continuariam se
achando durante a transição. É acréscimo, não bifurcação.

## O que ainda não foi provado

Que o canal 23 está livre nas duas máquinas da bancada, e que o vínculo sem registro SDP é aceito
pelas duas pilhas — em especial pelo Windows, cuja documentação de `AF_BTH` presume
`WSASetService`. É item da PoC-2, e esta decisão é a hipótese que ela vai testar. Se o Windows
recusar atender sem registro publicado, `WSASetService` volta — e só do lado do Windows, onde ele
não custa dependência nenhuma.
