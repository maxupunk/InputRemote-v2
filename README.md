# InputRemote 2

Um teclado e um mouse para dois computadores — inclusive na tela de bloqueio.

O ponteiro atravessa a borda da tela e o outro computador responde como se fosse mais
um monitor. Teclado e mouse viajam por **Bluetooth** quando ele existe, por **UDP** na
rede local quando não existe. Arquivos e imagens viajam por **TCP**, em conexão própria,
sem nunca disputar espaço com o ponteiro.

Windows e Linux/Wayland. Escrito em Rust.

> **Estado: nível N1 implementado.** O teclado e o mouse atravessam de um computador para o
> outro na sessão desbloqueada, com pareamento cifrado e comparação de um código de seis dígitos
> nas duas telas. Os três processos existem — serviço, agente de sessão e interface —, a janela
> pareia pelo próprio serviço, e os instaladores dos dois sistemas saem num comando.
>
> **O primeiro teste entre duas máquinas de verdade ainda não foi feito.** O que já foi
> exercitado, e o que falta, está em [USAR.md](USAR.md) e em
> [PROGRESSO.md](PROGRESSO.md); a tela de bloqueio (N2/N3) continua pendente.

> ⚠ **Risco aberto e conhecido.** Em janeiro de 2026 o Windows passou a recusar entrada
> injetada nas telas de credencial, salvo de teclado físico, de aplicação com UIAccess ou
> de aplicação com integridade elevada. O desenho deste projeto se enquadra na terceira
> categoria — e reforça a segunda —, mas isso ainda **não foi provado em hardware**. É a
> PoC-1, e ela é bloqueante do produto inteiro.
> Contexto em [docs/05-windows.md §4.4](docs/05-windows.md).

Duas consequências práticas disso, para quem for compilar:

- os binários do Windows precisam ser **assinados** e instalados em `%ProgramFiles%`, ou o
  UIAccess não é concedido e a digitação na tela de bloqueio não funciona;
- uma build sem assinatura serve para tudo o mais, inclusive uso normal — só não digita na
  tela de bloqueio.

### Níveis de capacidade

O requisito de entrada privilegiada é escalonado, e o produto declara qual nível alcançou
em cada sistema — na interface e aqui:

| Nível | Aceita entrada em | Situação |
|---|---|---|
| **N3** | tela de login, antes de qualquer usuário logado | alvo |
| **N2** | tela de bloqueio e diálogos de elevação | **piso: abaixo disto o produto não se justifica** |
| **N1** | apenas sessão desbloqueada | é um KVM comum |

Se a tela de login não for alcançável num sistema, entrega-se N2 ali, com a limitação
escrita — e não se bloqueia o resto. O que não é aceitável é o usuário descobrir o limite
na hora em que precisa dele. Ver [docs/01](docs/01-visao-e-escopo.md).

## O que muda em relação ao InputRemote 1

Este é um projeto novo, não uma refatoração. **Nenhuma linha do v1 é reaproveitada.**
Ele chegou a 23.500 linhas com 35% delas em dois arquivos, misturando interface, máquina
de estados, transporte e plataforma no mesmo processo. O que ele fazia bem está preservado
aqui como requisito; o que o tornou impossível de manter está documentado em
[docs/00-licoes-do-v1.md](docs/00-licoes-do-v1.md), proibido por regra em
[docs/09-padroes-de-codigo.md](docs/09-padroes-de-codigo.md) e listado nominalmente em
[docs/11-nao-legado.md](docs/11-nao-legado.md).

Quatro diferenças estruturais:

| | InputRemote 1 | InputRemote 2 |
|---|---|---|
| Processos | um só (GUI + captura + rede + Bluetooth) | três: serviço privilegiado, agente de sessão, interface |
| Tela de bloqueio | fora de escopo | requisito central — o serviço sobe com a máquina |
| Criptografia | SPAKE2 + TLS/QUIC, um caminho por transporte | Noise, um caminho só sobre Bluetooth, UDP e TCP |
| Lógica de produto | acoplada ao ciclo da GUI | núcleo *sans-io*, testável sem hardware |

## Como gerar os instaladores

Um comando, os dois sistemas:

```powershell
.\empacotar\empacotar.ps1
```

Sai em `dist/`: o `.msi` do Windows, assinado, e o `.rpm` do Fedora 44. O RPM é construído dentro
de um container do Fedora — não por compilação cruzada —, porque o gerador de dependências do RPM
precisa ler o ELF no próprio sistema de destino para acertar os `Requires`.

`-Alvo Windows` ou `-Alvo Linux` fazem só um lado. Detalhes e o que ainda **não** entra no pacote:
[docs/logs/11-instaladores.md](docs/logs/11-instaladores.md).

> A assinatura é autoassinada, para teste e uso local. Ela não substitui um certificado de
> verdade, e sem confiar nela na máquina de destino o Windows não concede `UIAccess` — sem o qual
> digitar na tela de bloqueio não funciona ([05, §4.4](docs/05-windows.md)).

## Como usar agora

Windows e Linux, nos dois sentidos, por Bluetooth e rede ao mesmo tempo, com pareamento cifrado;
copiar e colar texto, arquivos e imagens; pausa, atalhos e a saída de emergência. O passo a passo, e
o que ainda não foi provado em hardware, está em [USAR.md](USAR.md).

## Documentação

Comece pelo [índice](docs/00-indice.md).

## Licença

MIT, com a exceção documentada em [docs/07-stack-e-dependencias.md](docs/07-stack-e-dependencias.md)
sobre o licenciamento do Slint na interface.
