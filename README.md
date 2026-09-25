# InputRemote 2

Um teclado e um mouse para dois computadores — inclusive na tela de bloqueio.

O ponteiro atravessa a borda da tela e o outro computador responde como se fosse mais
um monitor. Teclado e mouse viajam por **Bluetooth** quando ele existe, por **UDP** na
rede local quando não existe. Arquivos e imagens viajam por **TCP**, em conexão própria,
sem nunca disputar espaço com o ponteiro.

Windows e Linux/Wayland. Escrito em Rust.

> **Estado: provado entre duas máquinas de verdade** — um Windows 11 e um Fedora 44, com os
> instaladores deste repositório. Os dois computadores controlam um ao outro: quem mexe no próprio
> mouse ou teclado manda, sem papel fixo de servidor e cliente. Bluetooth e rede funcionam ao mesmo
> tempo, com pareamento cifrado e comparação de um código de seis dígitos nas duas telas. Texto,
> arquivos e imagens atravessam copiando e colando.
>
> **Tela de bloqueio:** no **Fedora**, o Windows digita na tela de bloqueio (N2), e o Fedora
> bloqueado continua controlando o Windows. No **Windows**, digitar na tela de bloqueio ainda
> depende de o certificado do instalador ser confiado na máquina (o aviso abaixo).
>
> O que foi provado em bancada, e o que falta, está em [USAR.md](USAR.md),
> [PROGRESSO.md](PROGRESSO.md) e no [registro](LOG.md) — o mais recente é o
> [log 54](docs/logs/54-uma-regra-em-um-lugar.md). Ainda não provado em hardware: o ponteiro com
> dois monitores numa mesma máquina.

> ⚠ **Risco aberto e conhecido.** Em janeiro de 2026 o Windows passou a recusar entrada
> injetada nas telas de credencial, salvo de teclado físico, de aplicação com UIAccess ou
> de aplicação com integridade elevada. O desenho deste projeto se enquadra na terceira
> categoria — e reforça a segunda —, mas **no Windows** isso ainda não foi provado em hardware:
> é a PoC-1. No Linux, a tela de bloqueio do GNOME já recebe o que vem do outro computador.
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

## Por que Bluetooth e rede ao mesmo tempo

O teclado e o mouse não dependem do Wi-Fi. Quando os dois computadores têm Bluetooth, ele é o
caminho preferido, e a rede local entra junto como segundo caminho — a **rota dupla**. Cada
tecla segue pelos dois, e vale a que chegar primeiro.

- **Estabilidade.** O Bluetooth é uma ligação direta entre as duas máquinas: não passa pelo
  roteador, não disputa banda com downloads e não sofre com a economia de energia da placa de
  Wi-Fi, que faz o ponteiro travar e voltar.
- **O acesso não cai quando a rede muda.** Trocar de Wi-Fi, reiniciar o roteador, mudar o IP ou
  ficar sem rede não interrompe o controle: o Bluetooth continua, e a rede volta a somar quando
  reaparece. É o que permite, por exemplo, usar o teclado de um computador para reconfigurar a
  rede do outro.
- **E o inverso também.** Sem Bluetooth, ou com o rádio fora de alcance, a rede local assume
  sozinha, sem ninguém precisar escolher.

Arquivos e imagens, que são grandes, vão sempre pela rede (TCP), em conexão própria, para nunca
atrasar o ponteiro.

## Arquitetura

| | |
|---|---|
| Processos | três: serviço privilegiado, agente de sessão, interface |
| Tela de bloqueio | requisito central — o serviço sobe com a máquina |
| Criptografia | Noise, um caminho só sobre Bluetooth, UDP e TCP |
| Lógica de produto | núcleo *sans-io*, testável sem hardware |

As regras de código estão em [docs/09-padroes-de-codigo.md](docs/09-padroes-de-codigo.md).

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

[MIT](LICENSE).
