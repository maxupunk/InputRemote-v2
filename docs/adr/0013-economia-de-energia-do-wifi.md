# ADR-0013 — Avisar da economia de energia do Wi-Fi, e desligá-la com um botão — também no par

**Status:** aceito · **Data:** 2026-09-22

## O problema

Pela rede, o mouse fluía, travava e voltava a fluir — também com o Deskflow, e nunca pelo
Bluetooth. Na bancada ([log 44](../logs/44-o-wifi-que-cochilava.md)), o ping a cada segundo para o
Fedora teve picos de 190, 126, 99 e 67 ms, enquanto o roteador respondia em ~1 ms. O Wi-Fi do
Fedora estava com `Power save: on`: a placa cochila entre pacotes, e o ponto de acesso guarda o que
chega até o próximo *beacon*. Desligada a economia, o pior caso caiu para 12 ms.

A placa (Realtek RTL8852BE, driver `rtw89`) decide se acorda pela **vazão**, e os comandos de
entrada são pequenos demais para contar: ela cochila **mesmo com o mouse em movimento**. Não há o
que o produto faça no tráfego para mantê-la acordada; a correção é a configuração da placa.

## A decisão

1. **Cada serviço verifica a própria placa** — na subida e a cada 30 s, porque a economia volta a
   cada reconexão se nada a fixar — e conta ao par (`Control::NetworkPower`).
2. **A janela mostra o aviso com um botão "Resolver".** O aviso do par vem primeiro: quem olha esta
   tela é quem sente o mouse travar do outro lado, e a placa que atrasa o que ele manda é a do
   computador que recebe.
3. **O botão desliga a economia — daqui, ou pedindo ao par** (`Control::DisableNetworkPowerSaving`).
   No Linux, `iw dev <placa> set power_save off` agora, e
   `/etc/NetworkManager/conf.d/90-inputremote-wifi.conf` com `wifi.powersave=2` para sobreviver a
   reconexões. No Windows, o "modo de economia de energia" do adaptador sem fio no plano ativo vai a
   "desempenho máximo", na tomada e na bateria.

A detecção e a correção moram num crate de plataforma próprio, `ir-energia`, que não conhece nada
do produto. A leitura das saídas de `iw` e `powercfg` é pura e testada nos dois sistemas; a do
`powercfg` não lê texto, porque ele muda de idioma — lê os dois índices hexadecimais.

## Por que o par pode pedir

Ir até o outro computador para achar a configuração de energia da placa é exatamente o que o
usuário não quer fazer, e o outro computador pode nem ter a janela aberta. O pedido é seguro porque
é estreito ([04, §3.2.1](../04-seguranca.md)): só com sessão estabelecida (par autenticado com a
chave fixada), sem parâmetro nenhum, e só desliga a economia — o pior que um par legítimo faz com
ele é gastar um pouco mais de bateria.

## Consequências

- **Versão 3 do protocolo, com mínimo 2.** As mensagens novas só vão para um par da versão 3; um da
  versão 2 não as decodificaria, e no UDP uma mensagem confiável que nunca é confirmada derrubaria a
  sessão. Por isso a versão 2 continua aceita, e os dois lados não precisam atualizar juntos.
- **O RPM passa a exigir `iw`.**
- **O aviso só aparece com a economia ligada** — ou ligada só na bateria, com a frase dizendo isso.
  Um Windows na tomada com "desempenho máximo" e economia só na bateria mostra o aviso de bateria:
  é quando o usuário vai sentir, e resolver antes custa um clique.
- **Não se religa pelo produto.** Quem quiser a economia de volta apaga o arquivo do
  `NetworkManager` ou muda o plano de energia do Windows; o arquivo diz isso em comentário.
