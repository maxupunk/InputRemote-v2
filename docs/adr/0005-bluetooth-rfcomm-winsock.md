# ADR-0005 — RFCOMM por Winsock, e pareamento do sistema operacional manual

**Status:** aceito · **Data:** 2026-09-09 · **Substitui:** nada

São duas decisões, e elas se sustentam pelo mesmo motivo: o transporte precisa pertencer
a um serviço que sobe antes do login.

## Decisão A — Winsock `AF_BTH`, não WinRT

O v1 acessava Bluetooth por `Windows.Devices.Bluetooth` (WinRT). As APIs `Windows.Devices.*`
dependem de infraestrutura por usuário e não são um alvo suportado para serviços na
sessão 0. Como o v2 exige o enlace ativo antes de haver usuário, esse backend precisaria
ser reescrito de qualquer forma.

O v2 usa:

```text
socket(AF_BTH, SOCK_STREAM, BTHPROTO_RFCOMM)
WSASetService(...)                            publica o registro SDP
WSALookupServiceBegin/Next(...)               inquérito e busca de serviço
```

`AF_BTH` é uma família de sockets do kernel, sem dependência de infraestrutura por
usuário. No Linux, o equivalente é `org.bluez.ProfileManager1.RegisterProfile` pelo D-Bus
do sistema, a partir do serviço, com política de D-Bus própria do pacote.

A afirmação sobre WinRT é tratada como **hipótese a ser derrubada na PoC-2**, não como
fato estabelecido. A decisão por Winsock é segura de qualquer maneira: ela funciona em
serviço quer a hipótese esteja certa, quer esteja errada.

## Decisão B — o produto não dirige o pareamento Bluetooth do sistema

O usuário pareia as duas máquinas uma vez, pelas configurações de Bluetooth do próprio
sistema operacional. O produto:

- **não** registra `Agent1` no BlueZ;
- **não** chama API de pareamento do Windows;
- **não** exibe nem confirma o código de pareamento do sistema;
- oferece um botão "Abrir configurações de Bluetooth" e detecta quando o par aparece;
- se o par não estiver pareado, diz exatamente isso, com o passo a passo.

O pareamento do **produto** (código de seis dígitos, comparado nas duas telas) continua
existindo e é independente. São camadas diferentes, e agora só uma delas é nossa.

## Alternativas descartadas

**Dirigir o pareamento pelo aplicativo, como no v1.** Era o principal atrativo ("pareia
dentro do próprio aplicativo, sem abrir as configurações do sistema"), e foi a maior área
nunca validada: cinco itens em aberto no roadmap. Dirigir pareamento a partir de um
serviço na sessão 0 é ainda pior — não há onde mostrar diálogo, e no Windows depende
justamente das APIs WinRT descartadas na Decisão A.

**BLE/GATT em vez de RFCOMM.** Exige que um dos adaptadores atue como periférico, o que
não é garantido em hardware de PC com Windows, e a MTU pequena atrapalha. RFCOMM dá um
stream bidirecional confiável nas duas pilhas.

**HID over GATT (a máquina servidora se apresentando como teclado Bluetooth).** Seria
elegante: o cliente não precisaria de software nenhum e a tela de login funcionaria
nativamente. Inviável, porque o Windows não permite publicar o serviço HID (`0x1812`)
como periférico via GATT — e o servidor precisa ser Windows. Some-se a isso a
necessidade de um canal de volta para atravessar a borda de retorno.

## Consequências

**Boas.**
- O transporte funciona a partir da sessão 0, que é o requisito.
- Some a maior área de risco não validado do v1.
- As chaves de pareamento são da máquina nas duas plataformas
  (`HKLM\SYSTEM\...\BTHPORT\Parameters\Keys` no Windows, `/var/lib/bluetooth/` no Linux),
  então o par continua pareado na tela de login — que é o fato que torna o requisito
  possível.
- Menos código, e código mais simples de auditar.

**Ruins, e aceitas.**
- A primeira configuração tem um passo fora do produto. Mitigado por um botão que abre a
  tela certa e por detecção automática de quando o par aparece.
- `AF_BTH` é API antiga, com documentação escassa e ergonomia ruim em Rust; exige um
  módulo `unsafe` bem contido em `ir-bt::windows::winsock`.
- Diagnóstico de "por que não conecta" precisa distinguir "não pareado no sistema" de
  "pareado, mas o serviço não responde" — e dizer qual é.
