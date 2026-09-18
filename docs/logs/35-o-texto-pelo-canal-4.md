# O texto pelo canal 4, o alcance da confirmação, e o enlace que só um lado achava vivo

**Data:** 2026-09-18

**Itens:** Etapa 8 — texto atravessando; o serviço do Windows sabendo quem pediu o envio; o canal de
arquivos acompanhando o par sem reiniciar. Etapa 4 — a reconexão por UDP depois de uma queda.

**O que foi feito:** o que o [log 34](34-copiar-aqui-colar-la.md) deixou aberto, e três defeitos que
só apareceram fazendo isso.

## Texto

Pelo canal 4 da sessão, em qualquer portador, como o protocolo sempre previu: oferta com tamanho e
BLAKE3, pedido, pedaços, fim. Quem recebe confere tamanho e resumo e só então entrega
`Command::ClipboardText`; um pedaço fora de ordem ou além do anunciado abandona a montagem, e uma
oferta nova substitui a anterior no meio do caminho. O texto é um tipo (`ClipText`,
`TextoDoClipboard`) que não existe acima de 256 KiB e cujo `Debug` diz o tamanho, nunca o conteúdo
— o clipboard carrega a senha tirada do gerenciador de senhas.

**A cadência.** Sobre datagrama, janela cheia derruba o enlace (descartar perderia tecla), e sobre
Bluetooth o rádio é o mesmo do `KeyUp`. Sai um pedaço por batida de 5 ms (dois sobre UDP), com o
tamanho do menor portador, para uma troca de portador no meio não gerar quadro grande demais. Há
teste de que uma tecla é injetada no mesmo passo com 200 KB ainda na fila.

## O defeito que o texto achou na confiabilidade

O teste de 256 KiB com um quadro perdido a cada nove **derrubava o enlace**. Não era o texto: era a
camada de confiabilidade, desde sempre. A confirmação diz "a maior sequência que chegou, e quais das
32 anteriores". Se o emissor põe mais de 32 sequências à frente da mais antiga sem confirmação, esta
sai do alcance: o par a recebe, entrega, **e não tem como confirmá-la**, e o emissor a reenvia até
desistir e derrubar tudo. Contar pendentes não protege — cinco pendentes podem estar a quarenta
sequências uma da outra, se as do meio foram confirmadas.

`Sender::within_ack_reach` responde a pergunta certa, e o canal 4 a faz antes de cada pedaço. Com
isso o teste passa com até um quadro perdido em cada cinco. A digitação nunca chegou perto (32
teclas antes de uma retransmissão de 20 ms), mas a regra agora está escrita onde mora, com teste dos
dois lados dela.

## O enlace que só um lado achava vivo

Na bancada, depois de uma queda, a sessão nunca mais firmou por UDP. O Windows mantinha um enlace que
para o Linux já não existia e reabria sessão sobre ele a cada 3 s; o Linux discava, e o Windows
tratava o handshake dele como dado inválido. Três correções no `ir-net`:

1. **Reinício do par aceito** com o enlace de pé — mas só se o handshake novo terminar **com a mesma
   chave**. Um datagrama forjado com o endereço do par não derruba o enlace que funciona. Há teste
   para os dois lados, e o primeiro falha sem a correção.
2. **O iniciador ignora dado do enlace anterior** ainda em trânsito, em vez de tomá-lo por resposta
   ("datagrama malformado").
3. **De quem é a vez de discar** (`turno`): com os dois lados sabendo o endereço do outro, os dois
   discavam juntos e o iniciador lia o início do outro como resposta — em toda rodada, porque as
   rodadas têm o mesmo período. O de chave maior disca sempre; o de chave menor, na primeira e depois
   a cada três. O teste com os dois discando juntos nunca firma sem a regra (três de três) e firma com
   ela.

Na bancada, depois das correções, a sessão firmou de primeira e não caiu mais.

## O serviço do Windows sabe quem pediu

Como SYSTEM, o serviço recusava todo envio: o *pipe* não dizia quem estava do outro lado. Agora,
depois do primeiro pedido lido, ele captura o token do cliente (`ImpersonateNamedPipeClient`,
`OpenThreadToken` e `RevertToSelf` na mesma hora, sem `await` no meio) e, para cada arquivo **já
aberto**, pergunta ao Windows se aquele usuário poderia ler — `GetSecurityInfo` do handle e
`AccessCheck`. A ACL inteira decide; nada da regra é reimplementado.

A decisão que vale registro: **o token é de identificação, não de personificação.** Dá para
perguntar e não dá para agir como o usuário. Pedir mais ao cliente abriria o *pipe squatting* — um
programa que criasse um *pipe* com o nosso nome antes do serviço poderia agir como quem conectasse.

Um defeito saiu no caminho: no Windows, `File::open` não abre pasta sem `FILE_FLAG_BACKUP_SEMANTICS`,
e toda pasta copiada seria recusada antes de o sistema ser perguntado. O teste que o pega falha sem a
correção.

A interface fica no `ir-files` (`Autorizacao`, sem `unsafe`); a implementação, no `ir-acesso`.

## `ir-acesso`

O serviço passou do teto de 2 500 linhas de produção com a identificação do cliente. O limite apontou
uma fronteira que já existia em quatro módulos: **quem conectou ao serviço, e o que essa pessoa pode**
— o porteiro, a filiação a grupos no Linux, o SDDL dos *pipes* e a identificação do cliente no
Windows. Saíram juntos, com o histórico preservado.

E a régua tinha um defeito a favor do rigor: `#[cfg(test)] mod bancada;` contava como produção, 134
linhas no `ir-daemon`, contra a regra que ela mesma declara. O `xtask` agora tira da conta os módulos
de teste em arquivo próprio, com teste.

## Arquivos que acompanham o par

O canal de arquivos lia a chave do par uma vez, na subida: quem pareava ficava sem arquivos até
reiniciar o serviço, enquanto teclado e mouse já funcionavam. Agora o destino (chave e endereço) é um
`watch` que o serviço atualiza ao parear e ao esquecer, e o canal recomeça sozinho. Sem par, cada
pedido é recusado com o motivo. O teste de integração achou uma corrida no Linux — um pedido logo
depois de parear às vezes era recusado, porque o `select!` sorteava entre ele e a troca de par — e a
troca de par passou a ter prioridade.

## A prova

Windows (10.0.0.170) e Fedora 44 com GNOME (10.0.0.135), pela rede:

| Cenário | Resultado |
|---|---|
| Texto Windows → Linux, com acentos, travessão, emoji e CRLF | 68 B, SHA-256 igual, em LF no GNOME |
| Texto Linux → Windows | chegou, em CRLF no Windows |
| 218 890 B de texto, Windows → Linux | SHA-256 igual, 2,8 s da cópia ao clipboard, sessão sem queda |
| Sessão UDP depois das correções do `ir-net` | firmou de primeira e se manteve |

Na bancada, o Linux alternava entre a instância de teste e o serviço **instalado** do Windows pelo
Bluetooth — dois Windows com o mesmo nome. Foi desligado o rádio do Linux durante o teste
(`rfkill`), e religado ao fim.

## Arquivos

`ir-session` (`session/area.rs`, `event/clip_text.rs`, `within_ack_reach`), `ir-ipc` (`texto.rs`,
`OferecerTexto`, `TextoRecebido`), `ir-agent` (`clipboard/`), `ir-net` (`turno.rs`, reinício no
`endpoint`, filtro no `handshake`), `ir-files` (`Autorizacao`, `PeloSistema`, pastas no Windows),
`ir-acesso` novo, `ir-transferencia` (`Destino`), `ir-daemon`, `xtask` (módulos de teste em arquivo),
`empacotar/empacotar.ps1` (teto de CPU e memória para o container, depois de ele travar a máquina).

**Verificação:** 802 testes no Windows e o workspace no container do Fedora (`verificar.sh`),
clippy silencioso nos dois, `xtask` nos três critérios.

**O que ainda não foi provado:**

- A identificação do cliente com o serviço **instalado** (SYSTEM): exige instalar o MSI novo.
- O Ctrl+V à mão no Nautilus a partir do `text/uri-list`.
- Com a troca rápida de usuário no Windows, os ajudantes das duas sessões recebem o texto que chega.
  Cada um só publica na própria sessão, mas o que chega vai para as duas.
