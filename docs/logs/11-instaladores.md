# Instaladores: MSI, RPM e assinatura, num comando

**Data:** 2026-09-10

**Itens:** 1.5 — comando único, instalador `.msi`, RPM do Fedora 44, assinatura Authenticode,
manifesto.

**O que foi feito.** Um comando:

```powershell
.\empacotar\empacotar.ps1
```

produz em `dist/` o `.msi` do Windows, assinado, e o `.rpm` do Fedora 44. `-Alvo Windows` ou
`-Alvo Linux` fazem só um lado.

## O RPM sai de dentro do Fedora, não de compilação cruzada

Esta é a decisão que define o lado Linux. Um `.rpm` montado a partir de um binário cruzado teria
metadados inventados: o gerador de dependências do RPM lê o ELF e extrai os `Requires` de
verdade, e ele só acerta rodando no sistema de destino.

O resultado se vê na saída: o pacote declara `libfontconfig.so.1`, `libgcc_s.so.1(GCC_4.2.0)`,
`libc.so.6(GLIBC_2.39)` e mais uma dúzia de símbolos versionados — nenhum deles escrito à mão. Um
`Requires` escrito à mão erra, e erra na máquina de quem instala.

`empacotar/linux/Dockerfile` monta a imagem uma vez, com as dependências de compilação do Slint e
com **o mesmo `rustc 1.96.0` de `rust-toolchain.toml`**, e não com o Rust que o Fedora empacota.
Compilar o pacote com um compilador diferente do resto do projeto faria "funciona aqui" deixar de
significar alguma coisa.

A fonte entra no container **somente leitura**. O empacotamento não pode sujar a árvore de quem o
chamou, e um `target/` de Linux escrito por cima do de Windows seria uma tarde perdida.

A versão de desenvolvimento não cabe em `Version` — RPM não aceita `-` ali. Foi para `Release`
como `0.1.dev`, com o prefixo `0.`, que é a convenção de pré-lançamento: `0.1.0-0.1.dev` ordena
**antes** de `0.1.0-1`, então o dia do lançamento a atualização acontece sozinha.

## O MSI substituiu os scripts de instalação

`instalar.ps1` e `desinstalar.ps1` foram apagados. Duas maneiras de instalar a mesma coisa no
mesmo sistema é a bagunça que este projeto existe para evitar, e o MSI faz tudo o que eles faziam,
melhor:

- aparece em Configurações > Aplicativos, com desinstalação pelo caminho que o usuário conhece;
- `MajorUpgrade` remove a versão anterior antes de instalar a nova, em vez de deixar duas;
- `ServiceInstall`/`ServiceControl` registram e removem o serviço quando `inputremote-daemon.exe`
  existir — hoje o bloco é compilado condicionalmente e fica de fora.

E uma coisa que os scripts faziam **desnecessariamente**: o `icacls` por SID. Instalar em
`%ProgramFiles%` já herda a ACL que nega escrita a usuários comuns, que é o requisito do
`UIAccess`. O script estava reimplementando o que o Windows já garante — e, de quebra, estava
reimplementando errado, com nomes de grupo em inglês ([log 10](10-empacotamento.md)).

## A assinatura é autoassinada, e a janela não esconde isso

`empacotar/assinar.ps1` cria um certificado de assinatura de código na loja do usuário, com chave
**não exportável** — não existe `.pfx` em disco para vazar de uma pasta de build —, e assina os
executáveis e o MSI com o `signtool` do SDK do Windows.

Sem carimbo de tempo, de propósito. Carimbo serve para a assinatura sobreviver ao vencimento do
certificado, e um certificado de bancada que vence em três anos não tem nada a preservar; em
compensação, exigiria rede e quebraria a build quando a autoridade de carimbo estivesse fora do
ar.

Confiar no certificado é passo separado e explícito:

```powershell
empacotar\assinar.ps1 -Acao Confiar    # como administrador
empacotar\assinar.ps1 -Acao Remover    # para desfazer
```

Separado porque é uma decisão de segurança de verdade: instalar a parte pública em Raízes
Confiáveis da máquina faz **qualquer** binário assinado com aquela chave passar a ser confiável
ali. Enquanto não for feito, `Get-AuthenticodeSignature` devolve `UnknownError`, e o manifesto
escreve exatamente isso — "assinado, mas o certificado não é confiável aqui" — em vez de dizer
"assinado" e deixar o usuário concluir o que não é verdade.

Isso importa porque `UIAccess` exige binário assinado **e** confiável. Sem o passo de confiança, o
nível de capacidade fica em N1 ou N0 e digitar na tela de bloqueio não funciona
([05, §4.4](../05-windows.md)).

## Arquivos

`empacotar/empacotar.ps1` (orquestrador), `empacotar/assinar.ps1`,
`empacotar/windows/{Produto.wxs,LEIAME.txt}`,
`empacotar/linux/{Dockerfile,inputremote.spec,construir-rpm.sh,inputremote.desktop}`.
Removidos: `empacotar/{instalar,desinstalar}.ps1`, `empacotar/LEIAME.txt`.

## Verificação

**MSI.** Construído e assinado. Inspecionado sem instalar, pelo banco de dados do próprio pacote:
`ProductName=InputRemote`, `ProductVersion=0.1.0`, `UpgradeCode` igual ao do `.wxs`, quatro
arquivos e um atalho anunciado em `ProgramMenuFolder`. A tabela `ServiceInstall` não existe — o
que está certo, porque o serviço ainda não existe.

**RPM.** Construído, e depois **instalado e removido num `fedora:44` limpo**, que não é a imagem
de compilação:

- `dnf install` resolveu e puxou 13 dependências (fontconfig, freetype, harfbuzz, fontes);
- `ldd` não reporta nenhuma biblioteca faltando, nas 18 que o binário usa;
- o `.desktop` fica em `/usr/share/applications/`, o binário em `/usr/bin/`, a licença em
  `/usr/share/licenses/`;
- `dnf remove` sai sem resíduo.

**Comando único.** `empacotar.ps1` sem argumentos foi executado de ponta a ponta e produziu os
dois artefatos mais o `MANIFESTO.txt`.

## Quatro defeitos que a verificação achou

**`-out "$pasta\"` com espaço no caminho.** Um argumento que termina em barra faz o Windows tratar
a aspa de fechamento como escapada, e o `candle` recebia lixo. Só aparece quando o caminho tem
espaço — isto é, na máquina de outra pessoa. Passa o arquivo de saída em vez da pasta.

**O empacotador engolia a mensagem da ferramenta.** Quando o WiX falhava, o que saía era "light
falhou" — exatamente a informação que não ajuda. Agora toda ferramenta externa passa por
`Executar-Ferramenta`, que guarda a saída e a imprime quando o código de saída não é zero. Os três
defeitos seguintes só foram diagnosticáveis depois disto.

**Componente de MSI com três arquivos e GUID automático.** O MSI só gera GUID sozinho quando o
componente tem um único arquivo como chave. Um arquivo por componente.

**Atalho com chave de registro HKLM.** Disparava ICE38, ICE43 e ICE57 juntos. A construção certa é
o `<Shortcut>` dentro do `<File>` do executável: a chave do componente passa a ser o próprio
programa, e o Windows repara o atalho sozinho se ele sumir.

## O que continua de fora

**A unidade `systemd`, a regra `udev` e a política D-Bus** não entram no RPM ainda. Elas existem
para o serviço, e o serviço não existe; empacotar uma unidade que aponta para um binário ausente
faria o `systemctl enable` falhar na cara de quem instalasse.

**O RPM não é assinado com GPG.** Assinar exige uma chave do projeto, e uma chave gerada pelo
empacotador não provaria nada a quem instala — provaria apenas que o empacotador a gerou. Fica
para a Etapa 10, junto com o certificado de verdade do Windows.

**O WiX é baixado sob demanda** para `empacotar/ferramentas/` (fora do repositório), com o
SHA-256 conferido antes de descompactar. Era isso ou instalar o SDK do .NET na máquina de quem
empacota, o que é bem mais invasivo do que um zip verificado dentro da própria árvore.
