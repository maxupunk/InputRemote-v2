# 00 — Lições do InputRemote 1

Este documento existe para que os erros do v1 não sejam reimportados por hábito.
Cada item traz a evidência medida no repositório antigo e a regra que o substitui.

## 1. Dois arquivos concentravam um terço do produto

Medição do v1 (`crates/*/src`, 23.507 linhas no total):

| Arquivo | Linhas | % do produto |
|---|---:|---:|
| `inputremote-app/src/controller.rs` | 4.545 | 19,3% |
| `inputremote-app/src/application.rs` | 3.759 | 16,0% |
| `inputremote-platform/src/wayland.rs` | 2.110 | 9,0% |
| `inputremote-platform/src/windows.rs` | 1.460 | 6,2% |

O crate de interface (`inputremote-app`) tinha 10.491 linhas — mais que transporte
(4.891) e plataforma (5.542) somados. A interface havia virado o produto.

**Causa.** O crate da GUI recebeu, além do desenho da tela, a máquina de estados da
sessão, a orquestração de transportes, o clipboard e a configuração. Não havia uma
fronteira que impedisse isso, então cada recurso novo caiu no arquivo mais próximo.

**Regra nova.** [ADR-0004](adr/0004-nucleo-sans-io.md): a lógica de produto vive em
crates sem E/S, e a interface só sabe desenhar e falar IPC. Limites de tamanho por
arquivo e por crate são verificados pelo CI ([09](09-padroes-de-codigo.md)).

## 2. Processo único acoplou latência de entrada ao ciclo da interface

Captura, injeção, QUIC, Bluetooth, clipboard e a janela `eframe` corriam no mesmo
processo. Um `repaint` caro, um diálogo modal do sistema ou um travamento de driver
gráfico entrava no caminho do ponteiro.

**Regra nova.** [ADR-0001](adr/0001-tres-processos.md): a interface é um processo
separado, descartável, que pode ser fechado sem afetar a sessão.

## 3. A aposta em WinRT para Bluetooth teria sido fatal agora

O v1 acessava Bluetooth por `Windows.Devices.Bluetooth` (WinRT). As APIs `Windows.Devices.*`
dependem de infraestrutura por usuário e não são um alvo suportado para serviços na
sessão 0. Como o v2 exige que o transporte pertença a um serviço que sobe antes do
login, esse backend inteiro precisaria ser reescrito.

**Regra nova.** [ADR-0005](adr/0005-bluetooth-rfcomm-winsock.md): Winsock `AF_BTH` +
`BTHPROTO_RFCOMM`, que é uma família de sockets do kernel e funciona em serviço.
A hipótese sobre WinRT é tratada como **hipótese a derrubar na PoC-2**, não como fato:
a decisão por Winsock é segura mesmo que a hipótese esteja errada.

## 4. Bluetooth foi codificado sem nunca ter sido provado em rádio físico

O `ROADMAP.md` do v1 fechou a versão 0.1.0 com estes itens em aberto:

- [ ] Prova RFCOMM Windows↔Windows
- [ ] Prova RFCOMM Windows↔Linux
- [ ] Prova BLE/GATT e detecção de papéis
- [ ] Seleção automática do portador
- [ ] Fluxo Windows→Windows validado manualmente
- [ ] Fluxo Windows↔Linux validado manualmente

Havia 1.142 linhas de backend Bluetooth (`bluetooth_windows.rs` + `bluetooth_linux.rs`
+ `hybrid.rs`) escritas contra um comportamento nunca observado.

**Regra nova.** [Etapa 0](08-plano-de-implementacao.md): nenhuma etapa começa sem que
a prova de conceito correspondente tenha passado em hardware real, com critério de
aprovação escrito antes do teste. PoC reprovada muda a arquitetura, não o cronograma.

## 5. Três mecanismos de criptografia para o mesmo problema

O v1 carregava `spake2` (pareamento), `rustls` + `rcgen` + `tokio-rustls` (rede) e um
enquadramento próprio para RFCOMM. A promessa de "a mesma identidade vale em RFCOMM,
BLE e QUIC" (seção 8.4 do `PROJETO.md`) exigia costurar identidades entre modelos de
confiança diferentes — e o item "identidade do par fixada após pareamento" ficou
**não marcado** no roadmap. Ou seja: o pareamento acontecia, mas a identidade não era
fixada depois. Reconexão sem verificação forte é exatamente onde um KVM vira porta de
entrada.

**Regra nova.** [ADR-0003](adr/0003-noise-em-vez-de-quic.md): uma camada de cripto só
(Noise), idêntica sobre os três portadores, com a chave estática do par fixada no
pareamento e obrigatória em toda reconexão.

## 6. Fallback silencioso e modo "Híbrido" escondiam o estado real

O modo Híbrido decidia sozinho entre Bluetooth e rede, com regras diferentes por modo
("se o Bluetooth faltar, o Híbrido avisa e segue pela rede — o modo Bluetooth não faz
isso"). Três modos com três políticas de degradação produzem estados que o usuário não
consegue prever nem o desenvolvedor reproduzir.

**Regra nova.** O portador é sempre escolhido pela mesma política, e o estado corrente
(`portador`, `motivo da escolha`, `latência medida`) é observável na interface e no
diagnóstico. Ver [01, §5](01-visao-e-escopo.md).

## 7. Documentação de 53 KB em um arquivo

`PROJETO.md` tinha 21 seções e 53.374 bytes, misturando visão de produto, protocolo,
detalhe de API e riscos. Buscar uma decisão exigia rolar o arquivo inteiro, e o
documento envelheceu sem que ninguém percebesse quais partes já não valiam.

**Regra nova.** Documentos curtos por assunto, decisões em ADRs numerados e imutáveis.

## O que o v1 acertou e deve ser preservado

Nem tudo era problema. Estas escolhas se mantêm:

- separar canal de entrada (perdas toleráveis) de canal de dados (integridade obrigatória);
- exigir comparação visual de um código curto nos dois lados no primeiro pareamento;
- BLAKE3 para verificar cada arquivo transferido, com confirmação de volta à origem;
- limpar teclas e botões pressionados em toda falha e reconexão;
- atalho de emergência para devolver o controle;
- descoberta por mDNS com endereço manual como alternativa;
- `unsafe_code = "deny"` no workspace, com exceções explícitas;
- geração de pacotes Linux a partir do Windows por contêiner, com um comando.
