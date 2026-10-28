# A cópia que espera o cursor, e o ajudante do Fedora que não subia

**Data:** 2026-10-28 · **Branch:** `feat/painel-android`

**Itens:** [ADR-0011](../adr/0011-clipboard-na-travessia.md); [log 67](67-cancelar-a-copia-de-qualquer-lado.md).

**O que foi pedido:** "vamos avaliar a questão de, ao copiar, não ser enviado o arquivo para o outro
computador, somente quando o mouse for para a outra tela. Vamos verificar se está tudo funcionando e
se tem algum erro. Caso não tenha sido totalmente implementado, vamos concluir; caso já exista, vamos
implementar a opção na configuração do sistema."

## O que havia

- **A espera existia pela metade.** O log 67 fez a cópia **grande** (mais de 100 MB) esperar a
  travessia (`ir-agent/src/clipboard/grande.rs`). Texto, imagem e arquivo menor saíam na hora, e não
  havia como escolher outra coisa.
- **E um defeito que parava tudo no Fedora.** O registro do ajudante de clipboard mostrava, a cada
  3 s desde 16:38 (1 331 vezes), "o serviço mandou um quadro que este ajudante não entende". Ele saía
  achando que era de antes de uma atualização, o systemd o subia de novo, e o laço recomeçava. Nada do
  que se copiava no Fedora atravessava. O mesmo laço já tinha acontecido em 27/10, às 15:19.

## O defeito do Fedora

Uma sonda no canal de controle (só leitura: `Estado`, `Pastas`, `AcompanharClipboard`) trouxe os
quadros do serviço. O do estado tinha 152 B, e o decodificador atual parava com "fim do buffer". O
serviço e o ajudante falavam versões diferentes, mas o pacote instalado era um só.

A causa: o drop-in de bancada do log 66,
`~/.config/systemd/user/inputremote-clipboard.service.d/teste-portal.conf`, deixado de propósito à
espera da aprovação do portal. Ele trocava o ajudante do pacote por `~/ir-e2e/inputremote-agent`, de
23/10. Cada pacote novo atualizava o serviço, e o ajudante seguia o velho. O portal já está no pacote
desde o log 66, então o drop-in não servia mais para nada.

Feito: `portal-teste.sh desfazer`, que remove o drop-in e reinicia o ajudante do pacote. O binário
velho foi apagado. A ficha do portal (`~/.local/state/inputremote/portal-clipboard`) ficou. Depois
disso: `ExecStart=/usr/bin/inputremote-agent --clipboard`, `NRestarts=0` e nenhum quadro estranho.

O Windows não tinha o problema: o ajudante roda de `C:\Program Files\InputRemote` e estava de pé
desde 16:49.

## A opção

Preferências → **Copiar e colar** → **Quando enviar o que se copia aqui**:

| Escolha | O que acontece |
|---|---|
| **Na hora** | Sai ao copiar, para já estar pronto quando a pessoa chega ao outro computador. Arquivos acima de 100 MB esperam o cursor. É o comportamento de antes. |
| **Ao passar o cursor** (padrão) | Nada sai ao copiar. O que está copiado vai quando o cursor passa para o outro computador. |

O padrão foi escolha do usuário, depois de ver a opção: "vamos colocar 'Ao passar o cursor' como
padrão". Quem atualiza também passa a ele: um `config.toml` sem o campo sobe com "Ao passar o
cursor". O campo é sempre gravado, para a escolha de "Na hora" não se perder.

A frase abaixo da escolha muda com ela. O grupo some quando copiar e colar está desligado.

A opção vale **para este computador**: é o que sai daqui. Diferente de "copiar e colar", que os dois
combinam pela sessão, ela não muda o outro, e por isso não precisou de protocolo novo.

- **Contrato** (`ir-ipc`): `Estado.copia_na_travessia` no fim da estrutura, e
  `Pedido::CopiaNaTravessia(bool)` no fim do enum, com `Autoridade::Configurar` (o mesmo nível de
  "copiar e colar").
- **Configuração**: `copia_na_travessia` no `config.toml` (`serde(default = "sim")`), sempre gravado.
- **Ajudante**: `grande::Escolhas` junta "copiar e colar" e esta opção, e responde se cada gatilho lê
  o clipboard. Com "Ao passar o cursor", o aviso de cópia **nem lê** o clipboard: não abre o clipboard
  de quem acabou de copiar (o motivo do `ACOMODAR`) e não soma pastas. A travessia lê, e a guarda de
  eco garante que vá uma vez só.
- **Serviço**: só o pedido e o campo. Para caber no teto de 2 500 linhas, o retrato da janela
  (`ir-painel`) passou a ler as Preferências direto da configuração (`Retrato.config`), em vez de o
  serviço copiá-las uma a uma. Isso pede a seta nova `ir-painel → ir-configuracao`.
- **Arrumação no caminho**: a identidade da máquina saiu do `lib.rs` da configuração para
  `identidade.rs`, que tinha passado de 400 linhas. `BloquearJuntos` ficou numa linha, como os
  vizinhos.

## O que foi provado

- `clippy -D warnings` no workspace, `xtask check` dentro das regras e 300 testes dos crates tocados,
  todos passando no Windows.
- Testes novos:
  - com "Ao passar o cursor" (o padrão, igual ao do serviço recém-instalado), só a travessia lê;
    com "Na hora", os dois leem; desligado, nada lê;
  - o serviço nasce com "Ao passar o cursor", e "Na hora" fica gravado e volta na releitura;
  - um `config.toml` de antes sobe com "Ao passar o cursor", e o "Na hora" gravado não se perde.
- No Fedora, o ajudante do pacote de pé, ligado ao serviço e sem reinícios.

## O que falta

- Bancada com o pacote novo nos dois lados. Com "Ao passar o cursor": copiar um texto no Windows e
  conferir que nada sai (`oferecendo o clipboard ao par` não aparece no registro); passar o cursor ao
  Fedora e colar.
- O Fedora não foi recompilado nesta sessão: a mudança não tem código por sistema, mas o próximo
  `verificar.sh` confirma.
- Um ajudante que sobe incompatível com o serviço entra em laço para sempre, porque espera que o
  systemd suba o binário instalado. Isso só acontece com um drop-in de bancada, mas o aviso poderia
  dizer qual executável está rodando.
