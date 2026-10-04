# Copiar e colar, ligado ou desligado nas Preferências

**Data:** 2026-10-03

**Itens:** [ADR-0011](../adr/0011-clipboard-na-travessia.md) — o clipboard na travessia;
[log 60](60-a-copia-e-a-pasta.md) — a cópia e a pasta; [log 61](61-a-tela-inicial-mais-leve.md) — a
tela inicial; [PROGRESSO](../../PROGRESSO.md), Etapa 9.

**O que foi pedido:** uma opção "Copiar e colar" nas Preferências, ligada por padrão.
- **Ligada:** tudo fica como está — Ctrl+C num computador fica pronto para colar no outro.
- **Desligada:** copiar não faz nada no outro computador.

Com a melhor usabilidade, seguindo os princípios SOLID e DRY.

## O que a opção faz

Uma escolha deste computador, valendo para os dois sentidos:

| | Ligado (padrão) | Desligado |
|---|---|---|
| Ctrl+C aqui — texto, imagem, arquivos | vai ao outro computador | fica só aqui; o ajudante de clipboard nem lê o clipboard |
| Ctrl+C de dentro da pasta compartilhada | os caminhos vão pela pasta ([log 60](60-a-copia-e-a-pasta.md)) | não vão |
| Atravessar com o ponteiro (o gatilho do GNOME) | lê o clipboard e o leva | não lê |
| O outro computador copia texto | chega ao clipboard daqui | não chega |
| O outro computador copia arquivos | chegam | recusados antes de tocar o disco; lá aparece "copiar e colar está desligado no outro computador (Preferências)" |
| A pasta compartilhada | sincroniza | sincroniza do mesmo jeito: não é copiar e colar |

**Por que os dois sentidos.** Desligado só para enviar, o Ctrl+C do outro computador continuaria
trocando o clipboard daqui sem a pessoa pedir, e é justamente isso que quem desliga quer evitar.

**Por que recusar a cópia de lá, em vez de recebê-la e não colar.** Assim os bytes não atravessam à
toa, e quem copiou fica sabendo, no cartão da cópia e no aviso do canto, por que não chegou.

## Como, peça por peça

Uma decisão, num lugar: a chave mora no serviço, que é dono da configuração.

- **`ir-configuracao`.** `copiar_e_colar`, ligado por padrão. Um arquivo antigo, sem o campo, lê
  ligado.
- **`ir-transferencia/src/chave.rs` (novo).** `ChaveDaCopia`, uma chave compartilhada que nasce
  ligada. Quem a liga e desliga é o serviço; quem a consulta, sem guardar cópia:
  - o ator, para o que sai daqui;
  - a recepção de arquivos, para o que chega;
  - o canal das pastas, que a recebe junto com a `Faixa`.
- **A recepção reaproveita a recusa que já existia.** `Cota.permitido` já recusava com "não
  permitido" antes de qualquer gravação, e o outro lado já sabia contar isso. A chave entra ali, e
  nenhum motivo novo foi criado no protocolo. A frase de `Motivo::SemPermissao` passou a dizer onde a
  escolha se muda; hoje esta é a única recusa desse tipo.
- **`ir-daemon`.**
  - `Pedido::CopiarEColar(bool)`, no fim do enum, com a autoridade de "Bloquear juntos".
  - As duas preferências de ligar e desligar passam por uma função só, `definir_preferencia`: grava,
    aplica a chave e avisa a janela.
  - O que sai é segurado num ponto só (`copia_do_clipboard`): enviar arquivos, oferecer texto, ler na
    travessia e a cópia de dentro da pasta. Com a chave desligada, a resposta é "Feito", e não falha:
    não deu errado, foi a escolha da pessoa, e o ajudante não deve insistir.
  - O texto que chega e o "leia o clipboard" da travessia também consultam a chave.
- **`ir-canais`.** O que o outro computador copiou de uma pasta só vai ao clipboard com a chave
  ligada.
- **`ir-ipc`.** `Estado.copiar_e_colar`, no fim, com o padrão ligado.
- **`ir-agent`, o ajudante de clipboard.** Lê a chave do estado só para não trabalhar à toa: trazer
  uma pasta de rede para perto pode levar minutos. Quem decide continua sendo o serviço.
- **`ir-ui`.**
  - O cartão "Copiar e colar" em Preferências, com "Ligado / Desligado", logo depois de "Onde fica o
    outro computador". Os dois são os jeitos de os computadores trabalharem juntos.
  - A frase debaixo dos botões muda com a escolha e diz o efeito, inclusive que a pasta continua
    sincronizando.
  - As ações das chaves das Preferências saíram para uma função própria (`ligar_preferencias`).

## Verificação

- **Os testes novos:**
  - `ChaveDaCopia` nasce ligada, e quem tem um clone vê a mudança.
  - O serviço: ligado, enviar chega à transferência; desligado, fica gravado, a recepção também
    recusa, e enviar arquivos e oferecer texto não saem.
  - Duas máquinas de verdade pelo canal de dados (`ir-transferencia/tests/copia_desligada.rs`): com
    a chave desligada do lado que recebe, a cópia termina em "não permitido" e nada é gravado lá;
    religada, a mesma cópia passa.
- **Windows:**
  - `cargo xtask check`, `cargo clippy --workspace --all-targets -D warnings` e `cargo fmt` limpos;
  - `cargo test --workspace`: 1 331 passando, nenhuma falha.
- A tela, na demonstração (`--simulado --pareado`), fotografada:
  - Preferências com o cartão logo depois da borda, em "Ligado";
  - depois do clique em "Desligado", a frase muda para o efeito.
- **Não verificado ainda:** a opção com o serviço instalado entre as duas máquinas — o Ctrl+C real
  num lado, desligado, sem chegar ao outro.
