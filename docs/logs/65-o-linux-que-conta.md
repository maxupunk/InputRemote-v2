# O Linux que conta: a bandeja, o dock e a notificação que mostra o que chegou

**Data:** 2026-10-04

**Itens:** [log 64](64-copiar-e-colar-nos-dois.md) — o recado nativo e o ícone que conta, no
Windows; [PROGRESSO](../../PROGRESSO.md), Etapa 9.

**O que foi pedido:** "o novo fluxo no Windows ficou muito bom usando a notificação e a barra de
progresso; no Linux com o Wayland não seria possível melhorar? Vamos buscar formas nativas do
sistema."

## O que o GNOME não tem, e o que tem

Levantado antes de escrever código, e confirmado no notebook (Fedora 44, GNOME 50.5, Wayland):

| O que o Windows faz | No GNOME |
|---|---|
| barra de andamento dentro da notificação | **não existe.** O GNOME ignora a dica `value` (o KDE a desenha); nem a `GNotification` nem o portal têm barra |
| ícone na bandeja | o GNOME esconde ícones de programas. A extensão `AppIndicator` os mostra — o Ubuntu a liga de fábrica; o **Fedora a instala desligada** |
| janela de aviso no canto | o Wayland não deixa um programa escolher onde a janela aparece |
| — | **o dock desenha barra sobre o ícone do programa**: o Dash to Dock, o Ubuntu Dock, o Dash to Panel e o KDE atendem o sinal `com.canonical.Unity.LauncherEntry`, o mesmo do Firefox e do Nautilus. O notebook tem o Dash to Dock ligado |

Das quatro saídas apresentadas — o dock, a bandeja, uma extensão própria do GNOME Shell e só
melhorar a notificação —, a escolhida foi **bandeja + dock + notificação**. A extensão própria fica
para depois, se a bandeja não bastar: ela seria a mais nativa, mas pede manutenção a cada versão do
GNOME e um sair-e-entrar na sessão na primeira instalação.

## 1. O andamento no ícone do dock

`ir_recado::linux::doca`. A cópia em curso põe uma barra sobre o ícone do InputRemote no dock, e a
tira quando acaba.

- A conexão com o barramento fica de pé enquanto o ajudante viver. O dock apaga a barra de quem sai
  do barramento, então não serve um `gdbus emit` por atualização.
- Só manda quando a porcentagem inteira muda.
- Onde ninguém atende (o dock padrão do GNOME), o sinal se perde, sem custo.
- O `zbus` já estava na árvore (vem do Slint no Linux), com o mesmo motor (`async-io`): nenhuma
  biblioteca nova para isto.

## 2. O ícone na bandeja, igual ao do Windows

`ir_recado::linux::bandeja`, pelo `StatusNotifierItem`, o padrão que o KDE mostra sozinho e o GNOME
mostra com a extensão `AppIndicator`.

- **A mesma decisão e os mesmos quadros do Windows.** A `Vitrine` (o que acontece vira quadro) e os
  quadros decodificados saíram do código só-Windows para `ir_recado::bandeja`, e os dois sistemas
  usam o mesmo. O Windows só embrulha em `tray_icon::Icon`; o Linux, em ARGB, com os quatro tamanhos
  para o painel escolher.
- **Um quadro a cada 250 ms, e não a cada 100.** Cada quadro novo é um sinal que o painel redesenha,
  e o GNOME Shell é um processo só para tudo.
- **O menu diz o que está acontecendo,** numa linha apagada no alto ("Recebendo do outro computador:
  relatorio.pdf · 23 MB de 50 MB · 45%"), e tem **Abrir o InputRemote**. No GNOME o clique no ícone
  abre o menu, e o menu é onde se lê.
- **O ! some quando a pessoa mexe no ícone.** No Windows ele some quando a janela abre. Aqui a janela
  é outro processo e quase sempre está fechada, então clicar no ícone, ou abrir o menu, é o "vi".
- **Sem a extensão, o ícone espera.** Ele fica pronto (`assume_sni_available`) e aparece no instante
  em que a extensão é ligada, sem reiniciar nada.
- **Quem põe o ícone é o ajudante de clipboard**, e não a janela: no Linux a janela costuma estar
  fechada, e o ajudante vive a sessão inteira. Ele já recebia tudo o que importa: as cópias, o estado
  da conexão e as pastas.
- **A biblioteca é o `ksni`,** a implementação de referência em Rust, sobre o mesmo `zbus`. Ela faz
  o menu (`dbusmenu`), que é a parte trabalhosa do padrão. É de domínio público (Unlicense), mais
  permissiva que MIT, mas fora da lista do `deny.toml`. Entrou como exceção **só para ela**, com o
  motivo.

## 3. Ligar o ícone com um clique

No Fedora a extensão vem desligada, e sem ela o ícone fica pronto sem ninguém ver. Por isso:

- **O RPM recomenda a extensão** (`Recommends: gnome-shell-extension-appindicator`). O `dnf` instala
  as recomendadas por padrão.
- **As Preferências oferecem ligá-la** (`ir-ui/src/gnome.rs`). Um cartão "Ícone na barra superior"
  aparece só no GNOME e só quando falta algo:
  - **instalada e desligada:** a frase do que ela faz, e o botão **Ligar** (`gnome-extensions
    enable`), e o ícone aparece na hora;
  - **não instalada:** o nome do pacote, e que é preciso sair e entrar de novo na sessão;
  - **já ligada,** ou fora do GNOME: o cartão não existe.
- A pergunta ao GNOME leva alguns décimos de segundo, e roda fora do laço da janela.

## 4. A notificação, melhor dentro do que o GNOME deixa

- **A imagem que chegou vai em miniatura** (`image-path`): uma captura de tela se reconhece na hora.
  O Windows ganhou o mesmo (`<image>` na notificação).
- **"Abrir a pasta" abre com o arquivo já selecionado,** pelo `org.freedesktop.FileManager1.ShowItems`,
  que o Nautilus, o Dolphin e o Nemo atendem (o "Mostrar na pasta" dos navegadores). Sem quem
  atenda, cai para o `xdg-open` da pasta.
- Para isso o `Recado` passou a levar o caminho inteiro do que chegou (`recebido`), além da pasta.

## Como ficou o ajudante

O laço do ajudante de clipboard entrega todo aviso ao `Notificador` antes do `match`, e ele decide o
que a pessoa vê: a notificação, a bandeja e o dock. Os braços do `match` voltaram a cuidar só do
clipboard. Antes, a notificação estava espalhada em três braços.

## Verificação

- **Testes novos:**
  - os quadros: as quatro tiras abrem com os doze quadros do tamanho certo; o lado escolhe a tira; o
    arco gira e o resto fica parado;
  - a `Vitrine`: só pede quadro novo quando ele muda;
  - a bandeja do Linux: os quadros vão em ARGB, com todos os tamanhos;
  - o endereço `file://` escapa espaço e acento;
  - a linha do `notify-send` leva a miniatura;
  - o XML do Windows leva a miniatura;
  - a imagem que chegou é reconhecida pela extensão, e uma pasta não;
  - o cartão do GNOME: instalada e desligada oferece Ligar; ligada (inclusive a do Ubuntu) o cartão
    some; sem a extensão, diz o pacote.
- **Linux** (contêiner Fedora 44): clippy do workspace limpo; 1 365 testes passando, nenhuma falha.
  - Numa primeira rodada, um teste antigo do FUSE (`ler_alem_do_comeco_traz_o_arquivo_inteiro…`, em
    `ir-nuvem`, que esta mudança não toca) falhou por tempo e passou na rodada seguinte com o mesmo
    binário. Fica anotado como instável.
- **Windows:** clippy, `xtask` e `fmt` limpos; 1 359 testes passando, nenhuma falha.
- **No notebook de verdade** (Fedora 44, GNOME 50.5, Wayland, sessão ativa e desbloqueada), com
  `cargo run -p ir-recado --example vitrine` compilado no contêiner:
  - `gnome-extensions enable`, o comando do botão **Ligar**, deixou a extensão `ACTIVE`;
  - o ícone se registrou no GNOME (`RegisteredStatusNotifierItems` com o item do InputRemote);
  - o dock recebeu 22 atualizações, do 0% em diante, com a barra visível durante a cópia e escondida
    no fim (`dbus-monitor`);
  - as notificações saíram, do andamento à falha.
- **Não visto:** o GNOME só deixa a própria ferramenta fotografar a tela (`Screenshot is not
  allowed`), então a aparência no painel e no dock não foi conferida com os olhos. O binário ficou
  em `~/ir-e2e/vitrine` para isso.
- **A extensão `AppIndicator` ficou ligada no notebook.** Para desligar: `gnome-extensions disable
  appindicatorsupport@rgcjonas.gmail.com`.

## Depois: no Linux, a notificação só no fim

**O que foi pedido, depois de ver na tela:** tirar a notificação de andamento, porque "fica
piscando e chama a atenção", e deixar o andamento só com a extensão.

- **O motivo:** o recado de andamento se substituía a cada dois segundos, e o GNOME o mostrava de novo
  a cada substituição. É um aviso chamando a atenção para algo que não pede nada.
- **Agora, no Linux:**
  - a notificação aparece **uma vez**, no fim: chegou (com a miniatura e "Abrir a pasta") ou não
    atravessou;
  - e para o que o outro computador fez: a pasta oferecida, copiar e colar mudado;
  - o andamento fica no ícone da bandeja (girando, com a frase no menu) e na barra do dock, que não
    interrompem.
- **A regra mora num lugar só:** `ir_recado::linux::avisar` não mostra recado de andamento.
- **Com isso, o `notify-send` perdeu o que só existia para o andamento:** o recado que se atualiza no
  lugar (`--print-id`, `--replace-id`), o `--transient` e a dica `value`. O ajudante de clipboard
  não usa mais o `Ritmo`; ele fica só no Windows.
- **O Windows não muda:** lá a barra anda dentro da mesma notificação, sem reaparecer.

**A extensão vem com o pacote?** Vem, como recomendada (`Recommends`), e o `dnf` instala as
recomendadas por padrão. Ela não é obrigatória porque o pacote dela depende do `gnome-shell`, e
exigi-la puxaria o GNOME Shell inteiro para quem usa KDE, que mostra o ícone sem extensão. O pacote
não consegue **ligá-la**: ligar uma extensão é uma escolha de cada usuário, guardada nas
configurações dele. Para isso existe o botão **Ligar** nas Preferências.

**Verificação:**

- Windows: clippy e `fmt` limpos; os testes dos crates tocados passam.
- Linux (contêiner):
  - clippy do workspace limpo;
  - 1 363 testes passando e 1 falha, num teste antigo da trava de instância única do ajudante
    (`o_segundo_sai_e_a_trava_vai_embora_com_o_primeiro`), que esta mudança não toca;
  - rodado mais seis vezes em seguida, passou nas seis. Fica anotado como instável: ele usa um
    caminho com o número do processo numa pasta temporária compartilhada entre contêineres, onde os
    números se repetem.

