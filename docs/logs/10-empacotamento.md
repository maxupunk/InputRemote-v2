# Empacotamento: o que dá para instalar hoje

**Data:** 2026-09-10

**Itens:** 1.5 — empacotamento e pasta protegida (fechados); registro do serviço e desinstalação
sem resíduo (scripts prontos, execução pendente de elevação e do serviço).

**O que foi feito.** Os scripts que transformam o repositório em algo instalável, e a primeira
coisa foi acertar o nome: o executável passou a se chamar `inputremote-ui`, e não `ir-ui`. `ir-ui`
é como o código se organiza; `inputremote-ui` é como o usuário encontra o programa no menu
iniciar. [`docs/00-indice.md`](../00-indice.md) já tinha fixado os três nomes, e o binário estava
divergindo da própria convenção.

**`empacotar.ps1`** compila em release, junta os binários que existem, escreve um manifesto e
produz o `.zip`. O manifesto traz versão, commit (marcado como "árvore suja" quando há mudança
não comitada), SHA-256 e tamanho de cada arquivo, se está assinado — e **o que não entrou**. Um
pacote que omite o que falta é um pacote que mente, e quem o instalar vai descobrir a ausência na
tela de bloqueio, que é o pior lugar possível para descobrir.

**`instalar.ps1`** copia para `%ProgramFiles%\InputRemote`, restringe a ACL, cria o atalho e
registra o serviço — quando o serviço existir no pacote. Quando não existir, avisa em amarelo que
a interface vai abrir em modo de demonstração e que nada é digitado em computador nenhum.

**`desinstalar.ps1`** para e apaga o serviço, encerra os processos, remove a pasta e o atalho. A
configuração e o registro de diagnóstico ficam, salvo `-ApagarConfiguracao`: quem desinstala para
resolver um problema costuma reinstalar em seguida, e jogar fora o registro junto com o programa
apaga justamente o que explicaria a falha. É a mesma decisão do desinstalador da PoC-1
([log 07](07-poc1-tela-de-bloqueio.md)).

**Arquivos:** `empacotar/{empacotar,instalar,desinstalar}.ps1`, `empacotar/LEIAME.txt`,
`crates/ir-ui/Cargo.toml`.

**Verificação.** `empacotar.ps1` rodou de ponta a ponta: `cargo build --release` em 3 min 10 s, e
o pacote saiu em `dist/InputRemote-0.1.0-dev-windows-x64/` mais o `.zip`. O binário empacotado foi
**executado** a partir da pasta do pacote: a janela sobe, fica de pé e não escreve nada em stderr.
Os três scripts passam pelo analisador do PowerShell sem erro
(`[System.Management.Automation.Language.Parser]::ParseFile`).

**O defeito que a verificação achou.** A ACL estava escrita com nomes de grupo em inglês —
`Administrators`, `SYSTEM`, `Users`. Este Windows é em português, onde os grupos se chamam
`Administradores`, `AUTORIDADE NT\SISTEMA` e `Usuários`; o `icacls` teria falhado **depois** de os
binários já estarem copiados, deixando a instalação pela metade e a pasta com a herança de
permissões original. Trocado por SID — `*S-1-5-32-544`, `*S-1-5-18`, `*S-1-5-32-545` — e
verificado numa pasta temporária: o `icacls` sai com 0 e a ACL resultante é exatamente a
pretendida, com os nomes traduzidos.

O instalador também não estava copiando o desinstalador para a pasta instalada. Quem apagasse o
`.zip` ficaria com um serviço privilegiado registrado e sem o script que o remove.

**O que não foi verificado, e por quê.** `instalar.ps1` e `desinstalar.ps1` **não foram
executados**. Os dois exigem elevação — corretamente, porque escrevem em `%ProgramFiles%` e mexem
no gerenciador de serviços — e esta sessão não é elevada. Além disso, o caminho mais importante
dos dois, o registro e a remoção do serviço, não teria o que exercitar: `inputremote-daemon.exe`
ainda não existe. Esse caminho só fecha na Etapa 1.5 de verdade, junto com o binário.

Para testar à mão, num prompt como administrador, de dentro de
`dist/InputRemote-0.1.0-dev-windows-x64/`:

```powershell
powershell -ExecutionPolicy Bypass -File instalar.ps1
powershell -ExecutionPolicy Bypass -File desinstalar.ps1
```

**Decisões.** *Não* foi escrito instalador para Linux. A unidade `systemd`, a regra `udev` e a
política D-Bus estão desenhadas em [`docs/06-linux.md`](../06-linux.md), mas não há binário de
Linux para empacotar — o `slint` não cruza de Windows para Linux sem toolchain de destino — e um
instalador que ninguém consegue rodar não é entregável, é dívida disfarçada de progresso. O item
continua aberto no [PROGRESSO](../../PROGRESSO.md).

*Não* foi gerado MSI. Um MSI existe para colocar o produto na lista de programas instalados e
lidar com atualização e reversão, e as três coisas dependem de haver produto: serviço, agente e
assinatura. Enquanto o pacote tem só a interface, um `.zip` com script é mais honesto e mais fácil
de inspecionar — dá para ler o `instalar.ps1` antes de rodar, o que ninguém faz com um MSI.

**O aviso que fica.** Nada neste pacote está assinado, e o manifesto e o instalador dizem isso em
letras maiúsculas. Sem assinatura Authenticode e sem pasta protegida, o Windows não concede
UIAccess, e sem UIAccess a digitação na tela de bloqueio não acontece
([05, §4.4](../05-windows.md)). Para experimentar a interface isso não atrapalha; para o produto,
atrapalha tudo.
