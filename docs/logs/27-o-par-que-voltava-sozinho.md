# O par que voltava sozinho, e a latência que eu li errado

**Data:** 2026-09-16

**Itens:** Bluetooth de ponta a ponta pelo produto. Duas correções ao [log 26](26-o-bluetooth-que-nao-existia.md):
uma de leitura minha, outra de código.

## O Bluetooth funciona pelo produto, não só pela bancada

O log 26 terminou com quadros atravessando o rádio por uma ferramenta de bancada. Faltava o
produto: serviço, canal de controle, pareamento pedido de fora, e o estado que a janela mostra.

Sem privilégio de administrador, e sem tocar no serviço instalado — as três variáveis
`IR_DATA_DIR`, `IR_CONTROL_ENDPOINT` e `IR_AGENT_ENDPOINT` deixam subir uma segunda instância
inteira ao lado da primeira.

```text
Windows  CÓDIGO DE PAREAMENTO: 782731
Fedora   CÓDIGO DE PAREAMENTO: 782731
         PAREAMENTO CONCLUÍDO nos dois

ESTADO (Windows)
  enlace:   Pronto
  papel:    Servidor
  portador: Some(Bluetooth)
  motivo:   Some(Preferido)
  par:      Some(true)
```

**É a frase da reclamação, invertida.** Onde a janela dizia "Bluetooth indisponível; usando a
rede local", ela passa a dizer Bluetooth, e o motivo é `Preferido`. O caminho inteiro foi o de
produção: o daemon abriu o rádio, lançou o agente, subiu o canal de controle, recebeu
`IniciarPareamento` pelo IPC, discou por RFCOMM, e a sessão ficou `Pronto`.

Para pedir o pareamento sem janela nasceu o
[`ir-ipc/examples/controle.rs`](../../crates/ir-ipc/examples/controle.rs). Ele tem uma ação
`aguardar` que existe por uma corrida real: quem **recebe** o pareamento só tem o que confirmar
depois que o outro lado discou, e um `confirmar` mandado cedo demais recebe
`PareamentoInterrompido` e mata o pareamento.

## Correção 1: a latência não reprovava como escrevi

O log 26 registrou "reprova nos dois limites". **Está errado**, e a fonte é a tabela de metas de
[01, §6](../01-visao-e-escopo.md):

> | Latência **adicionada**, Bluetooth | mediana < 20 ms, p99 < 50 ms | carimbo monotônico na
> captura vs. injeção, relógios alinhados por ida-e-volta |

Duas coisas que eu não tinha lido com cuidado:

1. **A meta é de uma travessia** — captura numa máquina contra injeção na outra. A ida e volta
   aparece só como método de alinhar relógios, não como o que se mede. Os 49,84 ms e 90,02 ms
   que medi são ida e volta; por travessia dão **24,9 ms e 45,0 ms**. O p99 **passa**; a mediana
   falha por 25%, e não pelo fator 2,5 que meu texto sugeria.
2. **É latência *adicionada*, e o instrumento não mede isso.** Ele mede ida e volta de uma sonda
   no transporte. É um *proxy*, e registrá-lo como a linha da tabela fecharia a PoC-2 com um
   número que não é o que ela pede.

Quem apontou foi a sessão par; eu conferi na fonte antes de aceitar. Fica o lembrete de método:
**medir sem reler a definição da meta é medir outra coisa.**

O `R2` de [01](../01-visao-e-escopo.md) reforça a suspeita de *sniff* que o log 26 levantou —
previsibilidade vale mais que média baixa, e economia de energia do rádio ataca exatamente a
variância. A medida sob carga de 125 msg/s continua sendo o próximo passo, e pode sair **melhor**
que esta, porque com tráfego o enlace não fica ocioso.

## Correção 2: sem par gravado, a máquina aceitava qualquer um

Esta é de código, e é séria. Em `on_established`:

```rust
} else if let Some(fixada) = self.config.first_peer_key()
    && fixada != chave_do_par { recusa }
self.linked = true;
```

A recusa era "existe fixada **e** difere". Com `peers = []` não existe fixada, o ramo não
dispara, não há `else`, e cai direto em `linked = true`. Ou seja: **depois de "esquecer este
computador", bastava o par esquecido insistir num `Noise_IK` para voltar a ser aceito** — sem
código novo, sem confirmação visual, em silêncio. Contraria [04, §3.4](../04-seguranca.md).

E era isso que alimentava o laço visível no registro do Windows instalado: com `linked` ligado, a
reconexão reemite `CarrierUp` toda vez que a sessão vai a `Offline`, e só uma queda de transporte
zeraria `linked` — que nunca vinha. Foram 2,5 MB de registro num dia, e 23 min de CPU queimados
do outro lado.

A correção é uma linha, e a regra agora é positiva em vez de negativa: **sem pareamento em curso,
só passa quem já está fixado, e exatamente ele.**

```rust
} else if self.config.first_peer_key() != Some(chave_do_par) { recusa }
```

Três testes prendem o comportamento, e os dois últimos existem para a correção não trocar um
defeito por outro: sem par gravado ninguém entra e o enlace cai; com a chave fixada certa entra;
e com pareamento em curso entra e o par fica gravado.

O defeito é anterior a este trabalho — eu o transportei fielmente ao extrair o
[`enlace.rs`](../../crates/ir-daemon/src/actor/enlace.rs). Quem o encontrou foi a sessão par,
lendo os registros da bancada; eu confirmei na fonte antes de mexer.

**Verificação:** 33 testes no `ir-daemon` (eram 30), Windows e Fedora, clippy sem avisos.

## O que ainda não foi provado

- **Teclado e mouse atravessando** por Bluetooth. O que atravessou foi o pareamento e a sessão;
  a passagem de entrada depende do agente capturando, e não foi exercitada.
- **Latência adicionada** medida como a meta define, e sob carga de 125 msg/s.
- **O socket a partir do serviço**, na sessão 0. No Linux o rádio já abre pelo serviço systemd;
  no Windows a instância de teste roda em primeiro plano.
- **Windows↔Windows**, e o sentido Fedora→Windows pelo produto (pela bancada já foi).
- **Reconexão** depois de religar o rádio, e o par sobrevivendo a um reinício das duas máquinas.
