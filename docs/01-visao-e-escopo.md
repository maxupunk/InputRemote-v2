# 01 — Visão e escopo

## 1. O produto em uma frase

Dois computadores lado a lado, um teclado e um mouse só, com o ponteiro atravessando a
borda da tela — e o computador controlado obedecendo mesmo quando está bloqueado, para
que a própria senha de login possa ser digitada dali.

## 2. Os três requisitos que definem a arquitetura

Todo o resto é consequência destes três.

### R1 — O cliente DEVE aceitar entrada antes de haver um usuário logado

O software DEVE subir com a máquina, como serviço, e continuar funcionando na tela de
login, na tela de bloqueio e nos diálogos de elevação (UAC no Windows, `polkit` no Linux).

Desde a atualização de segurança de janeiro de 2026, o Windows só aceita entrada injetada
nas interfaces de credencial se ela vier de teclado físico, de aplicação com UIAccess ou
de aplicação com integridade elevada. Isso não é um detalhe de implementação: é o que
torna este requisito difícil e o que elimina o desenho ingênuo. Ver
[05, §4.4](05-windows.md).

Consequências inevitáveis:

- existe um processo privilegiado, permanente, que não pertence a nenhum usuário;
- o binário do agente **DEVE** ser assinado e instalado em local seguro — assinatura de
  código é requisito funcional, não boa prática ([04, §4.1](04-seguranca.md));
- esse processo processa dados vindos da rede e do rádio Bluetooth — logo, o modelo de
  ameaça de [04](04-seguranca.md) é o de um serviço exposto, não o de um aplicativo;
- a configuração e as chaves são da máquina, não do usuário;
- quem consegue falar com esse serviço consegue digitar a senha de administrador. O
  controle de acesso ao IPC é requisito de segurança de primeira classe, não detalhe.

### R2 — A latência de entrada DEVE ser previsível, não apenas baixa

Um ponteiro com mediana de 8 ms e p99 de 120 ms é pior de usar que um com mediana
constante de 20 ms. Por isso:

- Bluetooth é o portador preferido para teclado e mouse — ele entrega atraso constante e
  não disputa a Wi-Fi com o resto da casa;
- transferência de arquivo NÃO DEVE compartilhar portador com entrada, nunca;
- nada no caminho do evento de entrada pode alocar sob demanda, travar em `mutex`
  disputado, escrever log síncrono ou esperar a interface.

### R3 — A interface é acessório

A interface só configura, pareia e diagnostica. Fechá-la, matá-la ou nunca abri-la não
altera o funcionamento. Ela não é o dono de estado nenhum.

## 3. Escopo funcional

### 3.1. Entrada — obrigatório

- Movimento de ponteiro relativo e absoluto, com múltiplos monitores de geometrias
  diferentes nos dois lados.
- Botões (incluindo laterais) e roda vertical e horizontal.
- Teclado por posição física (*scancode*), com o layout do computador **controlado**
  decidindo o caractere. Modificadores viajam como estado, não como eventos soltos.
- Travessia de borda nas quatro direções, com retorno pelo lado oposto.
- Ctrl+Alt+Del no cliente Windows (via `SendSAS`, opcional na instalação — ver [05](05-windows.md)).
- Atalho de emergência que devolve o controle imediatamente e libera todas as teclas.
- Liberação garantida de teclas e botões em queda de enlace, troca de desktop, logoff,
  suspensão e encerramento do serviço.

### 3.2. Clipboard — obrigatório

- Texto UTF-8, bidirecional, canônico em LF no protocolo e nativo ao publicar.
- Imagem, canônica em PNG.
- Arquivos e diretórios, com manifesto, cota e verificação BLAKE3 por item.
- Sem laços de eco: uma cópia recebida não é reanunciada como cópia nova.

### 3.3. Transferência de arquivos — obrigatório

Conexão TCP própria, com a mesma identidade da sessão. Progresso e cancelamento
visíveis. Falha de transferência NÃO DEVE derrubar nem atrasar a entrada.

### 3.4. Operação — obrigatório

- Serviço com início automático, reinício após falha e desinstalação limpa.
- Descoberta por mDNS na rede local; endereço manual sempre disponível.
- Reconexão automática após queda, sem novo código de pareamento.
- Relatório de diagnóstico exportável, sem conteúdo sensível.
- Instaladores para Windows e pacote para Fedora/Debian.

## 4. Fora de escopo, e por quê

| Fora | Motivo |
|---|---|
| macOS | Nenhum caminho simples equivalente para a tela de login; custo de assinatura e notarização |
| X11 | Alvo Wayland; X11 já é bem servido por outras ferramentas |
| Android e iOS | Muda o modelo de Bluetooth e exige um aplicativo inteiro |
| Mais de um cliente | Multiplica a máquina de estados; reavaliar só depois de 1↔1 estável |
| Vídeo, áudio, tela remota | Outro produto |
| Acesso pela internet, relay, NAT traversal | Muda o modelo de ameaça por completo |
| Emulação de HID Bluetooth | Windows não publica o serviço HID (`0x1812`) como periférico; inviável no lado servidor |
| Arrastar e soltar visual entre telas | Depende de integração profunda com cada shell |
| Atualização automática | Um serviço SYSTEM que se atualiza sozinho é superfície de ataque; instalação é manual |

## 5. Política única de escolha de portador

O v1 tinha três modos com três políticas de degradação diferentes. O v2 tem **uma**
política, sempre a mesma, e a interface mostra o resultado dela.

Ordem de preferência para o canal de **entrada**:

1. Bluetooth RFCOMM, se pareado no sistema operacional nos dois lados e o enlace subir;
2. UDP na rede local;
3. nenhum — a sessão não estabelece e o motivo aparece escrito.

Canal de **dados** (clipboard grande, imagens, arquivos): sempre TCP na rede local.
Se não houver rede, esses recursos ficam indisponíveis e são anunciados como
indisponíveis. Eles NÃO DEVEM ser empurrados para o Bluetooth.

O usuário PODE fixar um portador ("usar somente Bluetooth", "usar somente rede"). Fixar
desliga o item 2 da lista, e a falha passa a ser falha — não degradação silenciosa.

Em qualquer momento, o estado corrente DEVE ser observável: portador ativo, por que ele
foi escolhido, latência mediana e p99 medidas na última janela de 10 s, e a última razão
de queda. Essa é a informação que faltou no v1 para diagnosticar qualquer coisa.

## 6. Metas de engenharia

São objetivos mensuráveis com método de medição definido em [10](10-testes-e-validacao.md),
não garantias contratuais.

| Métrica | Meta | Como se mede |
|---|---|---|
| Latência adicionada, Bluetooth | mediana < 20 ms, p99 < 50 ms | carimbo monotônico na captura vs. injeção, relógios alinhados por ida-e-volta |
| Latência adicionada, UDP em LAN | mediana < 8 ms, p99 < 25 ms | idem |
| Taxa útil do ponteiro | ≥ 125 Hz quando o hardware permitir | contagem de eventos injetados por segundo |
| Retorno após queda de enlace | ≤ 1 s até liberar teclas e devolver o controle | teste de desconexão forçada |
| Reconexão automática | ≤ 5 s após o par voltar | teste de queda e retorno |
| Tecla presa após 10.000 travessias | zero ocorrências | teste de estresse automatizado |
| Consumo do serviço em repouso | < 15 MB residentes, < 0,5% de CPU | medição em máquina ociosa por 1 h |

## 7. Critério de pronto

O produto está pronto quando as quatro combinações — Windows→Windows, Windows→Linux,
Linux→Windows, Linux→Linux — passam pelo roteiro de [10](10-testes-e-validacao.md) em
hardware físico, incluindo digitar a senha na tela de bloqueio do cliente, com os dois
portadores. Não há entrega parcial declarada como pronta.
