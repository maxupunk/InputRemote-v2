# Como usar

Um teclado e um mouse para dois computadores — Windows e Linux, em qualquer sentido. O ponteiro
atravessa pela borda da tela; o teclado vai junto; o que você copia de um lado cola do outro. A
conexão é por **Bluetooth e pela rede ao mesmo tempo** ([ADR-0012](docs/adr/0012-rota-dupla.md)),
cifrada com Noise, e o primeiro pareamento pede a comparação de seis dígitos nas duas telas.

## 1. Instalar

Os instaladores saem de um comando só, no Windows ([README](README.md#como-gerar-os-instaladores)):

```powershell
.\empacotar\empacotar.ps1
```

**Windows:** instale o `.msi` de `dist/`. Ele registra o serviço (sobe com a máquina, como SYSTEM),
libera o firewall e sobe o ajudante de clipboard na sessão aberta. O serviço lança sozinho o
**agente** dentro da sessão do usuário, e o ressobe se ele cair. Para digitar na tela de bloqueio e
no prompt de elevação, a máquina precisa confiar no certificado que assinou o pacote
([05, §4.4](docs/05-windows.md)).

**Linux (Fedora 44):**

```bash
sudo dnf install ./dist/inputremote-*.rpm
sudo usermod -aG inputremote "$USER"   # sem isto a janela não fala com o serviço
sudo systemctl enable --now inputremote
```

O grupo vale na hora, sem sair da sessão. O pacote também sobe o ajudante de clipboard nas sessões
abertas e instala o gancho de suspensão, que avisa o serviço antes de a máquina dormir.

## 2. Parear (só na primeira vez)

1. Abra o **InputRemote** nas duas máquinas.
2. Numa delas, **Parear** lista os computadores achados na rede e pelo Bluetooth. Escolha o outro.
3. As duas janelas mostram seis dígitos. Confira que são iguais e confirme **nas duas**. Diferentes
   significam que alguém pode estar no meio: recuse.

Uma máquina que já tem par só atende um pedido de pareamento de fora nos três minutos depois de
alguém abrir **Parear** nela; fora disso, o pedido é recusado sem aparecer na tela. Uma máquina sem
par atende sempre — é a primeira vez. A comparação dos dígitos não é pulável, nem pela tela, nem
por configuração.

Depois disso a conexão é automática: quem liga primeiro espera, e as quedas se refazem sozinhas.

## 3. Usar

Os dois computadores controlam um ao outro: não há "dono" do teclado.

- **Ir:** encoste o ponteiro na borda onde o outro computador está. Ele atravessa, e o teclado e o
  mouse daqui passam a controlar o outro. Vale dos dois lados.
- **Voltar:** a borda oposta — ou simplesmente **mexa no mouse ou no teclado do computador que está
  sendo usado**: ele retoma o controle na hora. Um esbarrão na mesa não conta; um clique, uma tecla
  ou um movimento de verdade, sim.
- **De que lado fica o outro** se escolhe na tela inicial de qualquer um dos dois; o outro passa a
  mostrar o lado oposto sozinho, e conta isso na tela.
- **Quem pode controlar** (Preferências): *Os dois* (o padrão), *Só este* (este controla o outro e
  nunca é controlado) ou *Só o outro*.

| Atalho | O que faz |
|---|---|
| **Ctrl+Alt+Shift+Espaço** | leva o controle ao outro computador, ou o traz de volta — dos dois lados |
| **Ctrl+Alt+Shift+Esc** | devolve o controle a este computador **e solta tudo** — a saída de emergência |
| **Ctrl+Alt+End** | o Ctrl+Alt+Del do outro computador |

Na janela:

- **Pausar** para de mandar entrada até **Retomar** — a conexão continua de pé.
- **Travar na borda** impede a travessia por acidente (num jogo em tela cheia, por exemplo); o
  atalho continua levando o controle.
- **Ctrl+Alt+Del no outro** faz o mesmo que o atalho.
- **Bloquear juntos** (Preferências): bloquear uma máquina bloqueia a outra.
- O **atraso** medido aparece no diagnóstico, com o gráfico do último minuto.

### Copiar e colar

Copie de um lado e cole do outro, com o Ctrl+C e o Ctrl+V de sempre. Atravessam:

- **texto**, pelo mesmo caminho da entrada (Bluetooth ou rede);
- **arquivos e pastas**, pelo canal de dados na rede, com o andamento no cartão de cópia (que tem
  **Cancelar** e **Abrir pasta**) — também de uma pasta de rede ou unidade mapeada, que é copiada
  antes para este computador (até 8 GB);
- **imagens** — uma captura de tela, uma figura copiada do navegador —, que chegam como imagem, e
  não como arquivo.

No Linux sem aviso de mudança de clipboard (o GNOME), o que foi copiado atravessa quando o ponteiro
atravessa ([ADR-0011](docs/adr/0011-clipboard-na-travessia.md)). Os arquivos recebidos ficam na pasta
de recebidos (Preferências mostra qual), que se limpa sozinha.

### Tela de bloqueio

No Windows, com o pacote assinado e confiado, o teclado e o mouse chegam também à tela de bloqueio,
ao Ctrl+Alt+Del e ao prompt de elevação. Se o administrador não permitir, a janela diz isso, e o
outro computador sabe que não deve atravessar enquanto esta tela estiver bloqueada.

## 4. Quando algo não vai bem

A janela diz o que está acontecendo e, quando há um botão que resolve, mostra o botão:

- **Travadas na rede:** é quase sempre a economia de energia do Wi-Fi. O aviso aparece com
  **Resolver**, que desliga a economia nas duas máquinas ([ADR-0013](docs/adr/0013-economia-de-energia-do-wifi.md)).
- **Tecla presa:** Ctrl+Alt+Shift+Esc devolve o controle e solta tudo. Se a conexão cair, o lado
  controlado solta tudo sozinho em até um segundo.
- **Serviço parado** (Windows): a faixa do topo oferece **Iniciar o serviço**.
- **Sem permissão** (Linux): a faixa mostra o `usermod` a rodar; a janela entra sozinha depois.
- **Diagnóstico** (Preferências) junta o estado das duas máquinas num texto para copiar e relatar.

O registro do serviço fica em `%ProgramData%\InputRemote\logs` (um arquivo por dia, sete dias) no
Windows e em `journalctl -u inputremote` no Linux. Ele registra tamanhos e tipos, nunca o que foi
digitado nem nomes de arquivo ([04, §7](docs/04-seguranca.md)).

## 5. O que ainda não foi provado em hardware

A lista honesta, do [log 45](docs/logs/45-a-varredura-implementada.md):

- a entrada na tela de bloqueio e no Ctrl+Alt+Del com o serviço **instalado** e o certificado
  confiado;
- o Linux **controlando** o Windows (captura por `evdev`) numa sessão GNOME;
- imagens no clipboard entre as duas máquinas (a conversão e o clipboard do Windows foram provados
  aqui; o Wayland e a travessia, não);
- a suspensão e a volta das duas máquinas com a conexão de pé.

O que foi provado na bancada está nos [logs](docs/logs/).
