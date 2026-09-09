# ADR-0003 — Noise sobre UDP, TCP e RFCOMM, em vez de QUIC

**Status:** aceito · **Data:** 2026-09-09 · **Substitui:** nada

## Contexto

O v1 escolheu QUIC (`quinn` + `rustls` + `rcgen` + `tokio-rustls`) para a rede, com o
argumento correto de que UDP cru não serve para tudo: perder um movimento de mouse é
aceitável, perder um `KeyUp` não é.

O argumento continua correto. O problema foi outro: **o Bluetooth não usa QUIC.** Então o
v1 acabou com dois modelos de segurança — TLS com certificados na rede, e SPAKE2 mais
enquadramento próprio no RFCOMM — e com a tarefa de costurar uma identidade única entre
eles. A promessa de "a mesma identidade vale em RFCOMM, BLE e QUIC" nunca se completou: o
item "identidade do par fixada após pareamento" ficou em aberto no lançamento.

Agora há um terceiro portador, o TCP de arquivos, e um requisito novo: tudo isso roda num
processo `SYSTEM` que sobe antes do login.

## Decisão

Uma camada de criptografia só, **Noise**, idêntica sobre os três portadores:

- `Noise_XX_25519_ChaChaPoly_BLAKE2s` no primeiro pareamento, com código de seis dígitos
  derivado do hash do handshake e comparado visualmente nos dois lados;
- `Noise_IK_25519_ChaChaPoly_BLAKE2s` em toda reconexão, com a chave estática do par
  fixada e obrigatória;
- em UDP, contador explícito como nonce e janela deslizante de 2 048 bits contra repetição;
- a confiabilidade do canal de entrada sobre UDP é nossa, e é pequena: sequência,
  confirmação cumulativa com bitmap e retransmissão ([03, §4.1](../03-protocolo.md)).

## Alternativas descartadas

**Manter QUIC na rede e algo próprio no Bluetooth.** É o v1. Dois modelos de confiança,
duas superfícies para auditar, e a identidade única virando trabalho de costura.

**QUIC também sobre RFCOMM.** QUIC pressupõe datagramas e caminho IP; forçá-lo sobre um
stream RFCOMM é adaptação sobre adaptação, com o controle de congestionamento do QUIC
brigando com o controle de fluxo do RFCOMM.

**TLS sobre TCP e nada sobre Bluetooth.** Inaceitável: o canal de entrada carrega senhas,
inclusive a da tela de login.

**Escrever a própria criptografia.** Nunca.

## Consequências

**Boas.**
- Um caminho de handshake, uma identidade, um lugar para auditar.
- Quatro dependências grandes a menos, num processo `SYSTEM`.
- O código de transporte fica genérico sobre "stream" e "datagrama"; o portador é detalhe.
- Repetição tratada explicitamente — num produto que digita senhas, reenviar tráfego
  gravado não é preocupação acadêmica.
- Chave fixada por construção; não existe caminho de reconexão sem verificação.

**Ruins, e aceitas.**
- Perdemos o controle de congestionamento do QUIC. Aceitável: o canal de entrada é de
  banda mínima e o canal de dados é TCP, que já tem o seu.
- Perdemos multiplexação de streams sem bloqueio de cabeça de fila. Aceitável: os canais
  de entrada são pequenos, e o canal de dados tem conexão própria.
- Perdemos migração de endereço. Aceitável: LAN, com reconexão em ≤ 5 s como meta.
- Escrevemos uma camada pequena de confiabilidade. É a parte que exige mais cuidado, e é
  por isso que ela está inteira em `ir-session`, sem E/S, com fuzzing obrigatório
  ([10, §3](../10-testes-e-validacao.md)).
