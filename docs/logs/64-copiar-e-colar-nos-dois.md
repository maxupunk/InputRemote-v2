# Copiar e colar vale para os dois; o recado nativo e o ícone que conta

**Data:** 2026-10-04

**Itens:** [log 62](62-copiar-e-colar-nas-preferencias.md) — a opção "Copiar e colar";
[ADR-0014](../adr/0014-controle-simetrico.md) — a borda, que já era uma escolha dos dois;
[log 37](37-a-bandeja-e-a-rolagem-de-lado.md) — a bandeja; [PROGRESSO](../../PROGRESSO.md),
Etapa 9.

**O que foi relatado:** com "Copiar e colar" desligado no Linux, um Ctrl+C no Windows fez aparecer,
**no Linux**, "A cópia não atravessou · … copiar e colar está desligado no outro computador
(Preferências)". Mas estava desligado no próprio Linux.

**O que foi pedido:**

1. Desligar de um lado desliga o outro, ao mesmo tempo, com foco em usabilidade.
2. Nos dois sistemas, avisos com as ferramentas nativas: no Windows, a dica do ícone da bandeja e
   um ícone animado que conte o que acontece. Melhor usabilidade, SOLID e DRY.

## Os dois defeitos que o relato mostrou

- **A escolha era de cada computador.** Desligado no Linux, o Windows continuava mandando, e a
  cópia só era recusada ao chegar. Para quem desligou, isso é exatamente o que não devia acontecer.
- **A frase da recusa era sempre a de quem envia.** A recusa acontece em quem recebe, mas as duas
  pontas mostravam a mesma frase. O Linux, que recusou, lia "desligado no outro computador". O mesmo
  valia para "não há espaço em disco no outro computador" e "passa do limite configurado no outro
  computador".

## 1. Uma escolha dos dois computadores

Mesma conversa da borda ([ADR-0014](../adr/0014-controle-simetrico.md)), que já era uma escolha dos
dois. Ela já tinha resolvido o problema das duas pontas discordarem.

- **Protocolo 9:** `Control::CopyPaste { enabled, chosen_at }`, no canal de controle.
  - Os dois lados anunciam ao estabelecer a sessão e a cada troca.
  - Se divergirem, vale a escolha mais recente.
  - No empate, vale **desligado**. É o caso de quem atualiza já com a opção desligada num lado:
    nenhum dos dois tem horário, e o desligado é o que alguém escolheu, porque o padrão é ligado.
  - As duas pontas fazem a mesma conta com os valores trocados, então exatamente uma cede.
- **Portão de versão**, como a pasta compartilhada: a mensagem só sai para quem negociou a versão 9
  (`version::supports_copy_paste`). Para um par da 8, mensagem desconhecida derruba a sessão. Com um
  par antigo, cada lado segue com a sua escolha, como antes.
- **`ir-session`** (`session/copy_paste.rs`): anuncia, compara e, quando a do par vale, adota e
  avisa (`Notice::CopyPasteAdopted`). Grava o horário do par, e não o de agora; senão esta ponta
  venceria a próxima comparação.
- **`ir-daemon`:**
  - mudar nas Preferências grava o valor com o horário (`copiar_e_colar_escolhido_em`, novo na
    configuração) e o passa à sessão;
  - a escolha adotada é gravada, aplicada à chave da recepção, e vira
    `Aviso::CopiarEColarAjustado { ligado, par }`.
- **Quem está do outro lado fica sabendo.** O Ctrl+C que para de atravessar sem ninguém mexer ali
  pareceria defeito. A notificação do sistema diz "Copiar e colar desligado — Desligado em NOTEBOOK,
  e vale para os dois…". A janela, se aberta, mostra a mesma frase no recado.
- **As Preferências dizem que vale para os dois:** "Mudar aqui muda lá."

**A frase da recusa** passou a ser do ponto de vista de quem lê: `Motivo::descricao(sentido)`.
Quem enviou lê "no outro computador"; quem recebeu e recusou lê "neste computador". Com a escolha
sincronizada, essa recusa quase não acontece mais. Mas acontece com um par da versão 8, ou se as
duas mudanças se cruzarem, e aí a frase precisa estar certa.

## 2. O recado nativo

**Antes:**

- no Linux, o `notify-send`, montado em quatro lugares de três crates (o ajudante de clipboard, a
  oferta de pasta e a queda da conexão), cada um com os próprios parâmetros;
- no Windows, uma janela própria no canto da tela (o "aviso flutuante"). Ela não respeitava o "não
  perturbe", não ficava na central de notificações e não tinha a cara do sistema.

**Agora, um crate novo, `ir-recado`.** Ele nasceu de uma fronteira comprovada: a interface passou
de 2 500 linhas de produção quando a notificação nativa e o ícone com estados entraram nela. Depende
só do vocabulário de `ir-ipc`. Sabe dar recado, e não sabe o que acontece. A seta
`ir-ui ──► ir-recado` foi aceita porque a regra que ela protege continua de pé, e o teste do `xtask`
passou a verificar a regra, e não a lista: tudo o que a interface alcança, por qualquer caminho, é
o vocabulário de `ir-ipc`.

| Peça | O quê |
|---|---|
| `Recado`, `Tom` | **o que dizer**, uma vez para os dois sistemas: a cópia em qualquer fase, a pasta oferecida, a conexão que caiu, copiar e colar mudado no par |
| `Ritmo` | **quando**: o fim aparece sempre e na hora; a cópia rápida (menos de 1 s) só tem o fim — um aviso, e não "copiando" seguido de "pronto"; depois, mudança de fase na hora, e o andamento a cada intervalo |
| `linux::NotifySend` | o `notify-send` num lugar só: um recado que se atualiza no lugar (`--replace-id`), o andamento passageiro (`--transient`, não enche o histórico do GNOME) e como dica `value` (o KDE desenha a barra), e **"Abrir a pasta"** (`--action`) no que chegou, esperado numa thread |
| `central::Notificacoes` | a central de notificações do Windows: a barra de andamento anda **no lugar** (`NotificationData`, sem reaparecer), "Abrir a pasta" pelo próprio Explorer (ativação `file:`), som só no que deu errado |
| `bandeja` | o ícone ao lado do relógio, abaixo |

**A notificação do Windows, sem pacote MSIX.**

- O InputRemote se registra em `HKCU\Software\Classes\AppUserModelId\InputRemote.InputRemote`, com o
  nome e o ícone que a central mostra.
- O registro é por usuário e é refeito a cada abertura: é barato e conserta um registro apagado.
- O ícone vai para `%LOCALAPPDATA%\InputRemote`, porque a central lê arquivo, e não recurso.
- Provado antes de escrever o código, com um identificador de teste, removido depois.
- O texto vai sempre pelos dados da notificação, e nunca dentro do XML. Um nome de arquivo com `<`
  ou `&` não é interpretado.
- Desligadas pela pessoa nas Configurações, nada aparece: ela escolheu.

**O aviso flutuante saiu.** A notificação nativa faz o que ele fazia, e mais. Se o Windows recusar
a central, o retorno fica no ícone da bandeja e na janela.

**Quem conta o quê** (`ir-ui/src/recados.rs`):

| Recado | Windows | Linux |
|---|---|---|
| a cópia de arquivos | a interface | o ajudante de clipboard |
| a pasta oferecida, copiar e colar mudado | a interface | os ajudantes |
| a conexão que caiu | a interface | a interface |

No Windows a interface mora na bandeja e está sempre de pé. No Linux a janela costuma estar fechada,
e quem vive a sessão inteira são os ajudantes.

## 3. O ícone que conta

O ícone da bandeja é o único pedaço do InputRemote sempre à vista. Ele ganhou um selo no canto, como
o do OneDrive:

| O que acontece | O ícone |
|---|---|
| uma cópia anda, ou uma pasta sincroniza (o tráfego do canal de dados) | o arco girando, um oitavo de volta a cada 100 ms |
| uma cópia não atravessou | o ! vermelho, até a pessoa abrir a janela ou copiar de novo — a notificação some sozinha, e a falha é a única coisa que pede ação |
| uma cópia chegou ou foi entregue | o ✓ verde, por 4 segundos — sucesso não pede nada |
| sem o outro computador, ou pausado | cinza |

**Como é feito:**

- **A decisão** (`bandeja::aparencia`) é separada do desenho e testada com o relógio de fora.
- **Os quadros** são desenhados pelo mesmo `recursos/gerar-icones.py` que é o ícone, com as cores de
  significado do tema.
- **Uma tira por escala de tela:** 16, 20, 24 e 32 px. Dar ao Windows um ícone grande para ele
  reduzir borraria o selo. A tira é escolhida pelo tamanho que o próprio Windows diz (`SM_CXSMICON`),
  porque a interface que sobe com o login começa escondida, e janela escondida não tem escala.
- **Os ícones de antes** saíram do gerador byte a byte iguais.

**A dica do ícone** fala da cópia enquanto ela anda, ou quando ela não atravessou. Por exemplo:
"Recebendo do outro computador — relatorio.pdf · 23 MB de 50 MB · 45%". Fora disso, como antes, o
estado da conexão.

## Verificação

- **Testes novos:**
  - **sessão:** desligar num lado desliga o outro com a sessão de pé, e religar religa; ao conectar
    vale a mais recente; sem horário dos dois lados vale desligado; escolha igual não gera aviso;
  - **serviço:** a escolha daqui grava o horário e chega à sessão; a do par é gravada com o horário
    dele, desliga a recepção e é contada;
  - **protocolo:** vetores gravados de `hello_v9` e `copy_paste`;
  - **frases:** a recusa diz "neste computador" a quem recusou;
  - **recados:**
    - o ritmo: a cópia rápida só tem o fim, o andamento a cada intervalo, a fase na hora, a mesma
      cópia de novo é outra;
    - a linha do `notify-send`;
    - o XML da notificação do Windows: o texto nunca entra nele, o endereço da pasta é escapado, e
      só o problema faz som;
  - **bandeja:** a aparência de cada caso; as quatro tiras abrem com os doze quadros.
- **`xtask`:** o teste da seta da interface verifica a regra (só o vocabulário de `ir-ipc` é
  alcançável) no lugar da lista.
- **Windows:** `cargo xtask check`, `cargo clippy --workspace --all-targets -D warnings` e
  `cargo fmt` limpos; `cargo test --workspace`: 1 352 passando, nenhuma falha.
- **Linux** (contêiner Fedora 44 com `/dev/fuse`): clippy limpo; 1 367 passando, nenhuma falha.
- **A cópia na central, de verdade** (`cargo run -p ir-recado --example vitrine`):
  - o primeiro recado aparece depois do primeiro segundo;
  - as atualizações seguintes voltam `Succeeded` do Windows, então a barra anda no lugar;
  - o fim toma o lugar do andamento.
  - Lido de volta da central, o último recado é "A cópia não atravessou · relatorio-anual.pdf ·
    copiar e colar está desligado neste computador (Preferências)", com som.
  - O histórico da central guarda os dados de quando o recado foi **mostrado**, e não os das
    atualizações: a atualização se confere pelo retorno, e não lendo o histórico.
- **A interface na demonstração** sobe com a bandeja nova e registra o nome e o ícone na central.
- **Não fotografado:** a sessão do Windows estava bloqueada durante a verificação (a captura sai
  preta, e a central mostra só "InputRemote · Bloqueado"). Falta ver a barra e o ícone girando na
  tela.
- **Não verificado ainda:** as duas máquinas com o serviço instalado (o Ctrl+C real com a opção
  desligada no Linux) e a notificação do GNOME com "Abrir a pasta".
