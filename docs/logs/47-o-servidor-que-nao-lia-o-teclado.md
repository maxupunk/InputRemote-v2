# O servidor que não lia o teclado, e a tela que mandava levar o ponteiro até a borda

**Data:** 2026-09-23

**Itens:** Etapa 6 — captura no Linux; Etapa 9 — a janela.

**O que foi relatado:** com os pacotes novos nos dois computadores e o Fedora como "Tem o teclado",
a tela do Fedora dizia *"Conectado a SAMSUNG-MAXUEL. Leve o ponteiro até a borda."*, com o selo
**N0** e o aviso *"O componente que digita nesta máquina não está pronto."* O ponteiro ia até a
borda e nada acontecia. O pedido: melhorar a experiência nesta situação.

## A causa

A unidade do systemd tinha `DeviceAllow=/dev/uinput rw`. Com qualquer `DeviceAllow`, o systemd passa
a **negar todos os outros dispositivos** ao serviço — e a captura do Linux
([06, §3.4](../06-linux.md)) lê o teclado e o mouse em `/dev/input/event*`. Ela não abria nenhum,
desistia, e o serviço seguia como servidor sem ter o que capturar. Nenhum teste via isto: só
acontece com o serviço rodando sob o systemd, e a captura por `evdev` era nova na varredura
([log 45](45-a-varredura-implementada.md)).

A tela piorava o problema em três lugares:

1. **"Leve o ponteiro até a borda"** com a captura parada — a instrução de fazer o que não ia dar
   certo.
2. **"O componente que digita"** no computador que tem o teclado — o que faltava era **ler** o
   teclado daqui, não digitar.
3. **A troca de papel era aceita sem testar a captura**, e a que vinha do outro computador também:
   a máquina ficava num papel em que nada funcionava, sem saber por quê.

## O que mudou

- **`DeviceAllow=char-input r`** na unidade: leitura da classe de entrada (major 13), sem escrita —
  o `EVIOCGRAB` funciona com o dispositivo aberto só para leitura.
- **Virar "Tem o teclado" testa a captura antes de gravar.** Se ela não abre, a troca volta com
  `Falha::SemCaptura` — *"este computador não consegue ler o teclado e o mouse ligados a ele"* e o
  que fazer —, e nada muda. A troca que vem do outro computador (protocolo 5,
  [log 46](46-os-papeis-que-combinam-sozinhos.md)) faz o mesmo teste e, se falhar, mostra a mesma
  falha em vez de adotar.
- **A captura se recupera sozinha**: com o teclado aqui e a captura parada, o serviço tenta de novo
  a cada ~9 s, em silêncio, e a tela sai do aviso quando ela sobe — um teclado USB que aparece
  depois, uma permissão dada depois.
- **As frases dizem o lado certo e o que fazer**: com o teclado aqui, *"Este computador não
  consegue ler o teclado e o mouse ligados a ele, então não controla o outro…"*; controlado,
  *"…não consegue receber o teclado e o mouse do outro…"*. O resumo passa a *"Conectado a X, mas o
  teclado e o mouse não atravessam."* — e, no controlado que funciona, *"O teclado e o mouse de lá
  controlam este."*, em vez de mandar levar um ponteiro que não é dele.

## Como foi verificado

- `ir-ipc/src/status/testes.rs`: o servidor sem captura ouve "ler o teclado e o mouse", o que fazer,
  e o resumo não manda levar o ponteiro.
- `ir-ipc/src/falha.rs`: `SemCaptura` no índice 17, com o que fazer.
- Os testes da troca de papel no serviço usam uma captura de mentira (`CapturaDeMentira`), para
  rodar também no container do Linux, que não tem teclado.
- 1 071 testes no Windows, 1 000 no container do Fedora; clippy e `xtask` limpos.

Falta a bancada: o Fedora como "Tem o teclado" com a unidade nova, atravessando para o Windows.
