# Copiar aqui, colar lá — e o serviço que podia ler demais

**Data:** 2026-09-18

**Itens:** Etapa 8 — `ir-clip` ligado ao produto; Ctrl+C e Ctrl+V de arquivos entre as duas
máquinas, provado nos dois sentidos.

**O que foi feito:** o ajudante de clipboard, a sincronização na travessia, o backend do Linux, a
correção de *confused deputy* no envio, e quatro defeitos do canal de arquivos que só apareceram
com duas transferências seguidas.

## O desenho: quem lê o clipboard é o usuário, não o serviço

A primeira ideia do log 33 era ligar o `ir-clip` ao agente. Não serve, e a
[ADR-0011](../adr/0011-clipboard-na-travessia.md) registra por quê:

- O agente roda como **SYSTEM** no Windows, e o clipboard é dado da sessão do usuário.
- No Linux, deixar um agente conectar desviaria a injeção do `uinput` — o canal do agente carrega
  entrada, e é `0600 root`.

Então é o mesmo executável, `inputremote-agent --clipboard`, rodando **como o usuário**, iniciado
com a sessão (`HKLM\...\Run` no MSI; `/etc/xdg/autostart` no RPM), falando pelo **canal de
controle** — o mesmo da janela, com a autoridade do usuário, e que nunca pode pedir injeção.

O fluxo:

```text
A: Ctrl+C no gerenciador de arquivos
A: ajudante vê a mudança          → Pedido::EnviarArquivos(caminhos)
A: serviço confere o leitor       → manifesto, blocos, BLAKE3 pelo canal 5
B: serviço publica na pasta de recebidos → Aviso::Transferencia{Concluida{destino}}
B: ajudante põe o destino no clipboard   → Ctrl+V cola
```

E a travessia: quando o controle passa para o outro computador (`Notice::ControlMoved{remote}`),
o serviço manda `Aviso::LerClipboard` e o ajudante reoferece o que está copiado. É o que cobre o
Linux do GNOME, onde não há como **vigiar** o clipboard — mas há como **lê-lo** na hora.

## O serviço que podia ler demais

O serviço do Linux é root; o do Windows, SYSTEM. Um usuário comum que pusesse `file:///etc/shadow`
no clipboard e cruzasse a borda teria o serviço lendo e enviando o que ele mesmo não pode ler —
o *confused deputy* clássico.

`ir_files::permissao::Leitor` resolve pela credencial de quem pediu, lida pelo transporte
(`SO_PEERCRED`) e nunca pelo que o pedido diz:

- `Proprio` quando o serviço não é privilegiado — o sistema já barra;
- `Usuario{uid}` quando é: o arquivo precisa ser do `uid` ou legível por outros, e **todo
  ancestral** atravessável por ele;
- `Desconhecido` quando não se sabe (o Windows como SYSTEM, até existir personificação do cliente
  do *pipe*): recusa com `Falha::SemPermissao`.

A conferência final é no **descritor aberto** (`fstat` + `/proc/self/fd`), não no caminho —
conferir o caminho e abrir depois deixaria a janela de troca por link simbólico.

## O que a bancada encontrou

Nada disto aparece com uma transferência. Aparece com a segunda.

1. **A segunda transferência falhava.** O `Verified` atrasado da primeira era consumido como
   resposta ao manifesto da segunda. Agora toda resposta é casada pelo `TransferId`, o velho é
   descartado, e a origem só declara concluído depois do `Verified` de **cada** arquivo.
2. **Impasse com fila limitada.** Com `bounded(32)`, o leitor esperava vaga para entregar
   respostas enquanto quem as consumiria esperava o leitor. As respostas são poucas e pequenas:
   fila sem limite.
3. **Cancelar com `TransferId(0)`.** O receptor ignorava — o `id` certo é o do envio em curso.
4. **Enlace morto sem ninguém perceber.** Com o canal ocioso, a tarefa de envio esperava pedidos e
   nunca soube que o leitor tinha saído. Agora espera pedido **e** o fechamento das respostas: o
   Linux reiniciado às 13:11:29, o Windows registrou a queda no mesmo segundo e religou às 13:11:36.
5. **`ProtectHome=yes`.** O teste Linux → Windows falhou com "não sei enviar
   `/home/maxuel/...`": o sandbox do systemd esconde `/home` do serviço, então **todo** Ctrl+C na
   pasta pessoal falharia. Passou a `read-only` — o `Leitor` decide o que pode ser lido, o sandbox
   garante que gravar ali continua impossível.

E duas do ambiente, que valem para quem testar de novo: `wl-paste`/`wl-copy` só funcionam dentro da
sessão gráfica (por SSH, `systemd-run --user`), e com a proteção de tela ativa o GNOME não entrega a
seleção — `gnome-session-inhibit` na bancada.

## A prova

Windows (10.0.0.170) e Fedora 44 com GNOME (10.0.0.135), pela rede:

| Cenário | Resultado |
|---|---|
| Windows → Linux, pasta com 2 arquivos, duas vezes seguidas | "envio concluído e conferido pelo destino arquivos=2" nas duas; `text/uri-list` no clipboard do GNOME; SHA-256 idêntico; o usuário lê o que chegou |
| Linux → Windows, `relatório do linux` (250 036 B) | 32 ms; nome acentuado intacto; `CF_HDROP` no clipboard do Windows; SHA-256 de `imagem.bin` `de6a12d2…` idêntico |
| Linux, `file:///etc/shadow` no clipboard | recusado: "sem permissão para enviar /etc/shadow"; nada chegou ao Windows |
| Linux reiniciado com o canal ocioso | o Windows percebeu na hora e religou em 7 s |

## Arquivos

`ir-ipc` (`Pedido::SincronizarClipboard`, `Aviso::LerClipboard`, `endereco_do_controle`),
`ir-daemon` (leitor por conexão, aviso na travessia), `ir-transferencia` (respostas por id,
conferência, queda percebida), `ir-files` (`permissao.rs`, manifesto e envio conferindo o leitor),
`ir-clip` (`uri.rs`, backend `wl-clipboard`, eco por travessia), `ir-agent` (`clipboard.rs`),
empacotamento (`inputremote-clipboard.desktop`, `Run` no MSI, `ProtectHome=read-only`,
`verificar.sh`), ADR-0011.

**Verificação:** workspace verde e clippy silencioso nos dois sistemas — o Linux pelo
`empacotar/linux/verificar.sh`, no container do Fedora, porque o código só de Linux não compila
aqui; `xtask` nos três critérios; os três cenários acima na bancada.

**O que ainda não foi provado ou não existe:**

- **Texto** não atravessa: o canal 4 (`ClipboardMessage`) ainda não é levado pela sessão.
- **Windows como origem, instalado:** o serviço como SYSTEM recusa enviar (`Desconhecido`) até a
  personificação do cliente do *pipe*. A prova Windows → Linux foi com a instância sem elevação.
- O canal de arquivos sobe com a chave fixada da subida; parear exige reiniciar o serviço.
- Na bancada, com o UDP, as duas pontas discam depois do pareamento e o portador de entrada cruza;
  a bancada voltou ao Bluetooth. Fica como defeito aberto.
- O Ctrl+V **à mão** no Nautilus a partir do `text/uri-list` que publicamos; e o GNOME com a
  proteção de tela ativa não entrega seleção, o que é do GNOME, não nosso.
