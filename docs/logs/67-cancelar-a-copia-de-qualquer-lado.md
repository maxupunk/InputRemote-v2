# Cancelar a cópia de qualquer lado, e a cópia grande que espera a travessia

**Data:** 2026-10-23

**Itens:** [ADR-0011](../adr/0011-clipboard-na-travessia.md) (revisto);
[PROGRESSO](../../PROGRESSO.md), Etapa 8.

**O que foi pedido:** "no Windows, quando copio, ele informa na notificação que está fazendo a
cópia; preciso da opção de cancelar, para evitar estar copiando para o próprio computador um arquivo
grande e o sistema ficar levando a cópia para o outro computador. Vamos verificar isso também no
Linux, buscar uma solução de melhor qualidade, com usabilidade e ótima experiência de usuário,
seguindo SOLID e DRY."

## O que havia

- **Cancelar existia, pela metade.** `Pedido::CancelarCopia` parava só a cópia que **sai** desta
  máquina. A que chegava não tinha como ser parada, e a janela só mostrava "Cancelar" para quem
  envia.
- **A notificação não tinha botão** nos dois sistemas. No Linux o andamento fica no ícone da bandeja
  e no dock, que também não ofereciam a ação.
- **A causa do caso relatado:** onde o sistema avisa a cópia, ela saía na hora. Copiar um vídeo para
  colar em outra pasta do mesmo computador o mandava inteiro para o outro. Com o portal do
  [log 66](66-o-clipboard-do-gnome-pelo-portal.md), o GNOME passou a fazer o mesmo.
- **E um defeito escondido:** a cópia cancelada voltava. O ajudante tratava o cancelamento como
  falha (`eco.oferta_falhou`), e a travessia seguinte mandava de novo o que a pessoa tinha acabado de
  parar.

## A decisão do usuário

Entre só pôr o botão, segurar toda cópia até a travessia, ou segurar só as grandes, a escolha foi a
terceira. **A cópia grande espera a travessia; o resto sai na hora; e tudo pode ser cancelado, dos
dois lados.**

## 1. A cópia grande espera a travessia

`ir-agent/src/clipboard/grande.rs`.

- No aviso de cópia (`Gatilho::Copia`), arquivos que somam mais de **100 MB** não saem.
- A guarda de eco não os marca como oferecidos, e a travessia (`Gatilho::Travessia`) os leva.
- A soma para no instante em que passa do limite, ou com mais de 10 mil itens, e não segue atalhos.
  Assim, decidir não atrasa a cópia que era pequena.
- Texto, imagem e o que está numa pasta compartilhada não passam por aqui: os dois primeiros são
  pequenos, e o último não manda bytes.

## 2. Cancelar dos dois lados (protocolo 10)

- **`Control::CancelCopy`**, no fim do enum e com portão (`version::supports_cancel_copy`). Quem
  recebe pede; quem envia cancela pelo caminho de sempre (`BulkMessage::Cancel`), e os dois lados
  descartam o que já foi.
- Na sessão: `Input::CancelPeerCopy` e `Notice::PeerCancelledCopy`, com
  `Session::can_cancel_peer_copy` para o serviço responder na hora quando não há a quem pedir.
- No serviço, `cancelar_copia`:
  - cancela aqui o que está saindo;
  - se nada está saindo, pede ao par;
  - se não há par que entenda, responde `ForaDeContexto`.
- A cópia cancelada não volta: `reagir` não libera a oferta quando o motivo é `Cancelada`. A
  resposta `ForaDeContexto` não é mais lida como oferta recusada.

## 3. Onde a pessoa cancela

Uma regra só decide se há o que cancelar: `Transferencia::cancelavel()` (a cópia acontece, de
qualquer lado). Uma ação só descreve o que se faz: `ir_recado::Acao::CancelarCopia`, com o rótulo, o
endereço e o pedido. Quem mostra apenas pergunta a elas.

| Onde | Como |
|---|---|
| notificação do Windows | botão **Cancelar** na notificação com a barra |
| bandeja do Windows | item **Cancelar a cópia**, apagado sem cópia em curso; aciona a mesma ação do botão da janela |
| bandeja do Linux | **Cancelar a cópia** logo abaixo da frase do que acontece, só enquanto a cópia acontece. No GNOME a notificação só aparece no fim, então é aqui que se para |
| janela | o botão **Cancelar** do cartão da cópia vale agora também para a que chega |

**O botão da notificação do Windows** abre `inputremote:cancelar-copia` pela ativação por protocolo,
o mesmo mecanismo do "Abrir a pasta" (`file:`).

- A interface se registra para o esquema a cada abertura (`central::Notificacoes::da_interface`,
  `HKCU\Software\Classes\inputremote`). O Windows a abre com `--acao <endereço>`, ela leva o pedido
  ao serviço e sai, sem janela e antes da marca de instância única.
- Funciona mesmo quando a pessoa clica no recado que ficou na central.
- O endereço só **pede**. Quem decide se há o que parar é o serviço, e uma página que abrisse
  `inputremote:` conseguiria, no máximo, parar uma cópia.

## Arrumação no caminho

- `Resposta::feito_ou(ok, falha)` substitui o `if ok { Feito } else { Falha(..) }` repetido no
  serviço.
- O ajudante ganhou `clipboard/pedido.rs`: que pedido leva cada conteúdo.
- Os testes do ajudante viraram uma pasta: `promessa.rs` e `travessia.rs`.
- Os vetores das versões 9 e 10 ficaram em `tests/vectors/copiar_e_colar.rs`.

## Verificação

- Windows: clippy do workspace limpo e todos os testes passando. `xtask check` dentro das regras.
- Linux (contêiner): clippy, testes e binários (`verificar.sh`).
- `inputremote-ui --acao inputremote:cancelar-copia` levou o pedido ao serviço instalado e saiu com
  0, sem janela.
- Testes novos:
  - a sessão leva o pedido ao par e não o devolve;
  - o serviço sem cópia e sem par responde `ForaDeContexto`;
  - o recado e o XML da notificação têm o botão só enquanto a cópia anda;
  - o endereço montado é o endereço lido, e endereço estranho não vira ação;
  - a cópia grande não sai no aviso e sai uma vez na travessia;
  - a cópia cancelada não volta.

**Falta, com as duas máquinas atualizadas** (o par precisa da versão 10 para quem recebe cancelar):

- clicar no **Cancelar** da notificação do Windows com uma cópia de verdade;
- cancelar pelo menu da bandeja do Linux a cópia que chega do Windows;
- copiar um arquivo de mais de 100 MB e conferir que ele só vai quando o cursor atravessa.
