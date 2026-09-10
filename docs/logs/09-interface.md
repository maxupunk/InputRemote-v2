# A interface: três telas, e o desenho no lugar da pergunta

**Data:** 2026-09-09

**Itens:** 1.3 — `ir-ui` (janela abre, mostra "sem par" e a impressão digital). Etapa 9 — tema,
telas, seletor de borda, estado observável, nível visível, serviço simulado, aviso de simulação,
diagnóstico.

**O que foi feito.** A janela de configuração em Slint, com três telas numa janela só e
navegação com voltar explícito em vez de abas. Das três, duas são visitadas uma vez na vida
(pareamento e preferências) e uma é visitada sempre; abas dariam o mesmo peso às três e fariam o
usuário escolher onde olhar.

**A tela inicial responde uma pergunta só: está funcionando, e se não, por quê?** A ordem dos
blocos é a ordem em que o usuário quer saber — estado primeiro, porque é o motivo de ele ter
aberto a janela; impedimento depois, porque é a única coisa que ele precisa resolver;
configuração por último, porque quase nunca muda.

**O seletor de borda desenha as duas telas.** Quatro botões de rádio escritos "esquerda /
direita / acima / abaixo" obrigam o usuário a traduzir palavra em espaço, e "esquerda" nunca
deixa claro *de quem*. Aqui ele vê um retângulo com o nome desta máquina e clica no lado onde a
outra está. A pergunta desaparece em vez de ser respondida.

**Os seis dígitos do pareamento aparecem grandes, separados, três e três.** Quem confere um
código lê em voz alta para a outra pessoa, e "104" separado de "782" é lido certo — um "104782"
corrido é lido errado. A tela também diz *por que* a conferência importa: um usuário que não
entende o motivo clica em "são iguais" sem olhar, e aí o passo todo deixou de existir.

**O nível de capacidade (N0 a N3) fica sempre visível**, num selo ao lado do estado, e não
escondido em preferências. É o que decide se o produto vai servir na tela de bloqueio, e
descobrir que não serve *com a tela bloqueada na frente* é o pior momento possível.

**Uma ação em destaque por tela, e um impedimento por vez.** Duas ações em destaque é o mesmo que
nenhuma; cinco avisos ao mesmo tempo é o mesmo que nenhum. `Estado::impedimento()` devolve no
máximo um, o mais grave, e a escala de saúde trata queda pedida pelo usuário como normal —
pintar de vermelho o que é normal ensina a ignorar vermelho, e aí a falha que importa passa
batida.

**Inversão de dependência, e honestidade sobre ela.** A interface fala com um `dyn Servico`, não
com um socket. `ServicoSimulado` é uma máquina de estados de verdade — descobre, espera, mostra o
código, conecta, mede atraso — que passa pelas mesmas transições e responde às mesmas mensagens.
Isso permitiu desenvolver, rodar e **testar** a interface antes de existir transporte. E a janela
mostra uma faixa dizendo que é demonstração: uma interface que finge estar funcionando é pior que
uma que não abre.

**O diagnóstico é mostrado, não copiado.** Num campo selecionável, com a instrução de copiar com
Ctrl+C. A área de transferência é do usuário, e escrever nela sem ele pedir apaga o que ele
tinha — num produto que compartilha clipboard, isso seria especialmente indelicado. O relatório é
montado por lista fechada de campos e tem teste verificando que não contém "tecla", "senha",
"clipboard", "caractere", "usage" nem "scancode": o produto vê senhas, e esse texto vai ser
colado em relatos públicos.

**Arquivos:** `crates/ir-ui/` — `ui/{tema,componentes,dados,bordas,inicio,pareamento,
preferencias,app}.slint`, `src/{lib,main,servico,ponte,simulado,janela}.rs`,
`tests/interface.rs`, `build.rs`.

**Verificação.** `cargo test -p ir-ui`: 22 testes passam — 12 em `ponte` e 10 conduzindo o fluxo
inteiro contra o simulado (procurar, comparar código, conectar, encerrar, esquecer, fixar
portador, desligar tela de bloqueio, diagnóstico). `cargo fmt --check`, `cargo clippy
--all-targets` e `cargo xtask check` limpos; 82 arquivos verificados. A janela foi **aberta**:
`target/debug/ir-ui.exe` sobe, fica de pé 8 s e não escreve nada em stderr.

**Decisão que custou uma refatoração.** `cargo xtask check` recusou `ir-ui → ir-proto`, e estava
certo: `docs/02 §2` diz "`ir-ui` ──► `ir-ipc` (e mais nada)". O defeito era meu — eu tinha posto
`Carrier`, `Edge`, `MachineId` e `PrivilegedInputLevel` na cara da interface através de
`ir_ipc::Estado`, de modo que a seta do Cargo era só o sintoma. Havia duas saídas: abrir exceção
na tabela, ou dar ao contrato vocabulário próprio.

Abrir exceção teria mantido a seta e perdido o que ela protege: se os tipos que a tela desenha
são os tipos do fio, mudar o formato de fio quebra a interface, e a interface passa a ter opinião
sobre protocolo. Então `ir-ipc` ganhou `vocabulario`, com `Portador`, `Borda`, `Nivel`,
`Maquina`, `Nome`, `Clipboard` e `Recursos`, e as conversões de e para o protocolo num lugar só.
O ganho visível: `Portador::nome()` devolve "Bluetooth" e `nome_tecnico()` devolve "RFCOMM", com
um teste garantindo que o segundo nunca vaza para a tela. O nome certo não diz nada a ninguém; o
nome da caixa do adaptador, sim.

`MachineName::coagido` foi acrescentado a `ir-proto` no caminho, com cinco testes. Um serviço que
se recusa a subir porque o hostname da máquina é comprido seria pior que um serviço com o nome
cortado — e o corte respeita limite de caractere, para não produzir UTF-8 inválido na máquina de
outra pessoa.

**Armadilha registrada.** `ir-ipc` nunca tinha passado por `cargo fmt`: o estilo compacto que eu
escrevia à mão não é o que a configuração do projeto produz. Eram treze arquivos divergentes, e o
CI teria falhado. A regra do CI é a autoridade, não o meu gosto.
