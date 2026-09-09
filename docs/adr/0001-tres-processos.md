# ADR-0001 — Três processos: serviço, agente, interface

**Status:** aceito · **Data:** 2026-09-09 · **Substitui:** nada

## Contexto

O requisito R1 ([01](../01-visao-e-escopo.md)) exige injetar entrada na tela de login e
em prompts de elevação. No Windows, isso só é possível a partir de um processo `SYSTEM`
amarrado ao desktop `WinSta0\Winlogon`, e um serviço na sessão 0 não pode fazê-lo
diretamente. No Linux, exige acesso privilegiado a `/dev/uinput`, antes de haver sessão.

O v1 tinha um processo só, com a janela `eframe` no mesmo espaço de endereçamento da
captura, da injeção, do QUIC e do Bluetooth. Nenhuma parte daquilo poderia rodar com
privilégio sem que tudo rodasse com privilégio.

## Decisão

Três processos por máquina:

1. `inputremote-daemon` — `SYSTEM` no Windows, usuário de sistema dedicado no Linux;
   permanente, dono de todo o estado;
2. `inputremote-agent` — dentro da sessão/desktop gráfico, descartável, sem estado;
3. `inputremote-ui` — sem privilégio, sob demanda, só configura.

O estado da sessão — inclusive quais teclas estão pressionadas — pertence ao serviço.

## Alternativas descartadas

**Processo único elevado.** Simples, mas exigiria que a interface Slint e o clipboard do
usuário rodassem como `SYSTEM`. Além de ser uma superfície de ataque enorme, um processo
`SYSTEM` não consegue interagir com o desktop do usuário de forma normal, e não resolve o
problema da troca de desktop.

**Serviço + biblioteca injetada no `winlogon.exe`.** Funciona, é o que alguns produtos
comerciais fazem, e é indefensável num projeto aberto: injeção em processo do sistema é
exatamente o comportamento que antivírus classificam como ataque, com razão.

**Driver HID virtual.** Resolveria a tela de bloqueio sem agente nenhum, e é tecnicamente
o caminho mais limpo. Exige assinatura WHQL com certificado EV, custo recorrente e
processo de submissão a cada versão. Fica registrado como alternativa caso a PoC-1
reprove ([08](../08-plano-de-implementacao.md)).

## Consequências

**Boas.**
- O privilégio fica confinado a dois binários pequenos.
- A interface pode travar, morrer ou nunca abrir sem afetar a sessão.
- A troca de desktop no Windows vira "matar e resubir um processo sem estado", que é uma
  operação segura, em vez de "realocar threads", que é impossível.
- A licença do Slint fica contida em um binário ([ADR-0007](0007-ui-slint-processo-separado.md)).
- Abre caminho, na Fase 2, para mover o decodificador de rede para um quarto processo sem
  privilégio.

**Ruins, e aceitas.**
- Um protocolo de IPC a mais para projetar, autorizar e manter.
- Autorização do IPC vira requisito de segurança de primeira classe
  ([04, §5](../04-seguranca.md)) — se qualquer processo do usuário puder pedir injeção, o
  modelo de segurança do Windows cai junto.
- Depuração atravessa fronteiras de processo; exige identificador de sessão correlacionado
  nos logs desde o primeiro dia.
- Instalação fica mais complexa: serviço, permissões e ciclo de vida do agente.
