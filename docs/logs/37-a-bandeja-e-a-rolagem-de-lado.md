# A bandeja, o ícone em branco e a rolagem de lado

**Data:** 2026-09-18

**Itens:** Etapa 9 — bandeja do sistema; ajustes de layout das Preferências; ícone do menu Iniciar.

**O que foi feito:** três pedidos do usuário, com a causa de cada um encontrada antes da correção.

## A rolagem horizontal nas Preferências

**Causa.** O `ScrollView` do Slint dá à área rolável a largura `max(largura visível, largura mínima
do conteúdo)`. Um único texto sem quebra de linha basta para passar da janela. Era a impressão
digital: oito grupos numa linha só, ao lado do rótulo, no `Dado`. Os cartões saíam cortados à
direita, com uma barra horizontal embaixo.

**Correção**, na causa e na estrutura:

- a área rolável tem a largura da parte visível (`viewport-width: self.visible-width`), então
  rolagem horizontal deixa de existir, qualquer que seja o conteúdo;
- o valor do `Dado` quebra linha em vez de empurrar a largura;
- a janela passou de 420 para 460 px, com folga para o seletor de três opções;
- o selo do nível (N1/N2) esticava ao lado da explicação. No Slint, `Text` tem esticamento 0 por
  padrão, e a sobra era dividida com o selo. Agora quem estica é a explicação.

**Como foi visto:** um roteiro abre a interface em modo simulado, acha o botão pela árvore de
acessibilidade (UI Automation), clica por mensagem à própria janela e fotografa com `PrintWindow`,
com a janela atrás das outras e sem clique em outro programa. As fotos de antes e depois mostram os
cartões cortados e depois inteiros.

## O ícone em branco no menu Iniciar

**Causa.** O atalho do MSI é *anunciado* (`Advertise="yes"`), e atalho anunciado não lê o ícone do
executável: ou declara um, ou fica com a folha em branco. O `.lnk` instalado tinha `IconLocation`
vazio. O `<Icon>` existia no instalador, mas só na lista de Programas.

**Correção:** `Icon="icone.ico"` no atalho.

## A bandeja

No Windows, a interface agora mora ao lado do relógio:

- **clique no ícone** abre a janela; o botão direito abre o menu (Abrir / Sair);
- **minimizar e fechar escondem** na bandeja; só "Sair" encerra. Fechar esconde também porque, senão,
  o ícone sumiria justamente onde o usuário o procura. Isso não afeta nada: a sessão é do serviço;
- **uma interface por sessão:** abrir pelo menu Iniciar com ela já na bandeja traz a janela existente,
  por um evento nomeado em `Local\` (por sessão, então a troca rápida de usuário não mistura as
  interfaces). É o único `unsafe` do crate, confinado em `bandeja/instancia.rs`;
- **sobe com o login**, já na bandeja (`--bandeja`, em `HKLM\...\Run`);
- **o MSI encerra a interface** antes de trocar os arquivos (`util:CloseApplication`). Fechar a janela
  só a esconde, e sem o Restart Manager o executável preso só seria trocado depois de reiniciar.

Fora do Windows nada muda: o GNOME não tem bandeja por padrão.

O ícone é o `tray-icon`, o mesmo do Tauri, e reusa o ícone embutido no executável (recurso 1).

**Verificado por mensagens do Windows**, contra a interface real:

| Passo | Resultado |
|---|---|
| aberta com `--bandeja` | processo vivo, janela escondida |
| segunda abertura | a primeira apareceu, e a segunda saiu sozinha |
| minimizar | escondida, processo vivo |
| terceira abertura | reapareceu |
| fechar no X | escondida, processo vivo |

O Explorer registrou o ícone (`NotifyIconSettings`, dica "InputRemote").

**O que não foi provado:** o clique de verdade no ícone. Simulado por mensagem, ele é descartado pelo
próprio `tray-icon`, que pergunta ao Windows onde o ícone está e, no Windows 11, não tem resposta
enquanto o ícone está na área de ícones ocultos. Só a mão no mouse responde.

## Arquivos

`crates/ir-ui` (`bandeja.rs`, `bandeja/instancia.rs`, `main.rs`, `janela.rs`, `lib.rs`,
`ui/app.slint`, `ui/tema.slint`, `ui/componentes.slint`, `ui/preferencias.slint`, `Cargo.toml`),
`empacotar/windows/Produto.wxs`, `empacotar/empacotar.ps1` (extensão `WixUtilExtension`).

**Verificação:** 802 testes, clippy silencioso, `xtask` nos três critérios; a interface compilada
no Fedora pelo RPM; as fotos e a tabela acima.
