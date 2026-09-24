# Os dois controlam um ao outro também na tela de bloqueio

**Data:** 2026-09-24

**Itens:** [04, §6](../04-seguranca.md) — a política da tela de bloqueio;
[ADR-0014](../adr/0014-controle-simetrico.md).

**O que foi relatado:** com o Fedora na tela de bloqueio, o mouse do Fedora ia para o Windows e o do
Windows não passava para o Fedora — agora parando na borda, pela mudança do
[log 52](52-a-tela-de-bloqueio-que-virava-parede.md). O pedido: que isto seja híbrido e automático,
os dois controlando um ao outro.

## A causa

A mesma do log 52, por outro caminho: o Fedora tinha sido pareado de novo às 18:36, e todo par novo
nascia com `tela_de_bloqueio = false`, o padrão de segurança de então. O registro do Fedora dizia
de novo *"digitação do par na tela de bloqueio recusada: a permissão está desligada"*. Funcionava
como projetado; o projeto é que não era o que se queria.

## A decisão

**A permissão nasce ligada para o par pareado.** O par só existe depois da comparação dos seis
dígitos nas duas telas, e só ele, com a chave fixada, chega à tela de bloqueio. Quem quiser proibir
desliga em Preferências, por computador. A seção 6 da segurança foi revista com o motivo.

## O que mudou

- **O arquivo grava a recusa, e não a permissão** (`recusa_tela_de_bloqueio`, desligada e fora do
  arquivo por padrão). O campo antigo, `tela_de_bloqueio`, é ignorado: um par gravado antes passa a
  poder, sem precisar parear de novo nem mexer nas Preferências.
- **No Windows, a política de Ctrl+Alt+Del acompanha sozinha**: com a permissão valendo, o serviço
  liga `SoftwareSASGeneration` na subida e ao parear, e grava o valor anterior para devolvê-lo se a
  permissão for desligada. Antes isso só acontecia clicando em Permitir.
- **O agente fica sabendo logo depois do pareamento**, e não só na próxima vez que conectar.
- `contar_ao_agente_a_permissao` passou a usar `protegido_permitido`, em vez de repetir a pergunta.

## Como foi verificado

- `ir-configuracao`: um par gravado com `tela_de_bloqueio = false` sobe permitindo; o padrão não vai
  ao arquivo; uma recusa escolhida é gravada e relida.
- Os testes do log 52 continuam valendo para quem desligar: a borda vira parede, e o aviso chega na
  hora.
- 1 102 testes no Windows, 1 045 no container do Fedora; clippy e `xtask` limpos.

## O que depende do hardware

No Linux, o `uinput` chega à tela de bloqueio do GNOME e ao GDM. No Windows, digitar na tela de
bloqueio e no Ctrl+Alt+Del exige o agente como SYSTEM com UIAccess — o MSI assinado **e o
certificado confiado** na máquina ([05, §4.4](../05-windows.md)). Sem o certificado confiado, o
Windows bloqueado continua sem receber o que vem do Fedora; a tela do Fedora diz isso.
