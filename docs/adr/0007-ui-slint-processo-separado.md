# ADR-0007 — Interface em Slint, em processo separado

**Status:** aceito · **Data:** 2026-09-09 · **Substitui:** nada

## Contexto

A interface do v2 só configura, pareia e diagnostica ([01, R3](../01-visao-e-escopo.md)).
Ela não participa da sessão.

No v1, `eframe`/`egui` rodava no mesmo processo que a captura, a injeção e os transportes,
e o crate da interface acabou com 10.491 linhas — mais que transporte e plataforma
somados. A janela virou o dono da lógica porque estava no mesmo lugar que ela.

## Decisão

`inputremote-ui`, binário separado, sem privilégio, escrito em Slint, dependendo apenas de
`ir-ipc`. Ele não conhece `ir-session`, `ir-net`, `ir-bt` nem `ir-input`, e o CI verifica
isso ([09, §2](../09-padroes-de-codigo.md)).

A interface **não lê nem escreve** arquivo de configuração. Ela pede ao serviço, que é a
única fonte da verdade — eliminando as corridas de escrita concorrente do v1.

## Alternativas descartadas

**`egui`/`eframe`, como no v1.** Rápido de escrever e sem dependência de sistema, mas o
modelo imediato empurra estado para dentro do laço de desenho, que foi exatamente o
mecanismo do acoplamento. Em processo separado o risco seria menor, mas o resultado visual
é limitado para uma tela que o usuário vai usar para parear e diagnosticar.

**Tauri 2 / WebView.** Máxima liberdade visual. Custo: `webkit2gtk` vira requisito de
instalação no Linux para uma janela de configurações, e o repositório passa a ter duas
stacks e duas cadeias de build.

**GTK ou Qt nativo.** Melhor integração no Linux, dependência pesada no Windows, e duas
interfaces para manter se quisermos nativo dos dois lados.

## Consequências

**Boas.**
- Fechar, matar ou nunca abrir a interface não altera a sessão em nada. É requisito, e
  agora é estrutural.
- Nenhum crate do produto liga contra o Slint — a licença fica contida em um binário.
- Binário pequeno, sem WebView, sem dependência de sistema no Linux além do que o próprio
  ambiente gráfico já tem.
- Declarativo: a descrição da tela fica em `.slint`, longe da lógica.

**Ruins, e aceitas.**
- **Licença.** O Slint não é MIT. O produto é. A decisão registrada em
  [07, §3](../07-stack-e-dependencias.md): `ir-ui` usa a licença livre de royalties do
  Slint e exibe a atribuição exigida num item "Sobre"; o resto do repositório permanece
  MIT. Se a licença do Slint mudar de forma inaceitável, troca-se um binário — não o
  produto. Essa contenção é uma das razões concretas para o desenho de três processos.
- Ecossistema menor que o de GTK, Qt ou web; menos componentes prontos.
- Todo dado exibido precisa atravessar o IPC, então o protocolo de estado precisa ser
  projetado, não improvisado.
- A bandeja do sistema vem de outra biblioteca (`tray-icon`) e tem comportamento variável
  no GNOME sem extensão — o produto funciona igual sem ela, e avisa.
