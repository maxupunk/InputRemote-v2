# A senha pedida pela janela, e não um comando de terminal

**Data:** 2026-09-18

**Itens:** Etapa 1.5 — unidade `systemd` no Linux; Etapa 9 — a janela diz o motivo e o que fazer.

**O que foi feito:** depois de instalar o RPM, a janela abria dizendo *"O serviço não está
respondendo… Suba o serviço com `sudo systemctl enable --now inputremote`"*, com um cartão de estado
vazio embaixo (um ponto verde sem texto e um selo em branco) e um botão de parear que não tinha como
funcionar. A primeira experiência com o produto era um comando de terminal — e, resolvido esse, viria
outro (`usermod`) na tela seguinte.

## As causas

1. **O pacote não habilitava nem subia o serviço.** Não havia `%post`, `%preun` nem `%postun`. Pelo
   mesmo motivo, a remoção deixava o serviço rodando um binário apagado — foi preciso pará-lo à mão
   na limpeza do log anterior.
2. **Ninguém punha o usuário no grupo `inputremote`**, que é o que abre o canal de controle. Com o
   serviço de pé, a janela trocaria "serviço parado" por "sem permissão", com outro comando.
3. **A janela sem serviço mostrava as telas que dependem dele**, vazias.

## A decisão: o que não precisa de decisão acontece sozinho; o que precisa, é pedido pela janela

O `dnf` e a loja de programas não fazem perguntas na instalação, e não devem: um pacote que para à
espera de resposta trava atualização automática. Então:

- **O pacote** habilita o serviço pelo mecanismo de *presets* do systemd (`80-inputremote.preset`,
  que um administrador ainda pode vetar com um preset próprio), o inicia na primeira instalação,
  reinicia na atualização só se estava rodando, e para e desabilita na remoção.
- **A janela** oferece um botão — "Liberar o acesso", ou "Ativar o InputRemote" se o serviço estiver
  parado — que chama `pkexec /usr/libexec/inputremote/ativar`. O diálogo é o do próprio sistema, e a
  mensagem é a da ação `io.github.inputremote.ativar` do polkit: diz o que vai mudar e por quê, e
  que é só uma vez. A frase na janela avisa da senha **antes** de o diálogo aparecer.
- **O ajudante** faz duas coisas fixas: põe no grupo quem pediu e garante o serviço habilitado e
  ligado. Quem pediu vem do `PKEXEC_UID`, que o `pkexec` define e o chamador não escolhe; o ajudante
  não aceita argumento nenhum. A janela nunca vê a senha nem roda nada como root.
- **Sem serviço, a janela mostra só o que resolve:** um cartão com o que aconteceu, o que vai mudar e
  o botão. O resultado de uma tentativa que não deu certo aparece embaixo, com o motivo
  (cancelada, não autorizada, ou o código do ajudante).

**A permissão vale na hora.** O serviço confere o grupo no banco de usuários a cada conexão
(log 17), então não é preciso sair da sessão. E a janela não espera os dez segundos que normalmente
separam as tentativas depois de uma recusa de permissão: quando a ativação dá certo, ela pede uma
tentativa imediata (`Servico::tentar_agora`). Na bancada, antes disso, a janela levou dez segundos
para entrar depois do ajudante — o bastante para parecer que a senha não tinha adiantado.

**O texto padrão da política é o português**, o idioma da janela: a tradução do polkit depende do
agente de autenticação, e o pedido de senha não pode sair num idioma diferente da tela que o
provocou. O inglês fica como variante marcada.

## A prova, no Fedora 44 da bancada

| Passo | Resultado |
|---|---|
| instalação limpa | serviço `enabled` e `active` |
| `pkaction --action-id io.github.inputremote.ativar` | reconhecida, `auth_admin`, caminho do ajudante |
| ajudante chamado sem `pkexec` | recusa, código 64 |
| janela aberta com o usuário fora do grupo | o serviço registra "interface recusada" — o estado em que a janela mostra "Liberar o acesso" |
| ajudante como o `pkexec` o chama (`PKEXEC_UID=1000`) | usuário no grupo, serviço ativo; a janela aberta entrou sozinha, sem reiniciar nada |
| remoção | serviço `inactive`, fora do boot, nenhum processo |

No Windows, o cartão sem serviço foi fotografado (sem o botão, que só existe onde há polkit).

**O que não foi provado:** o diálogo de senha de verdade, com a mão no teclado. Ele depende do agente
de autenticação da sessão gráfica, que não se alcança por SSH.

## Arquivos

`empacotar/linux/` (`ativar`, `io.github.inputremote.ativar.policy`, `80-inputremote.preset`,
`inputremote.spec`), `crates/ir-ui` (`ativacao.rs`, `ui/ativacao.slint`, `ui/app.slint`,
`ui/dados.slint`, `janela.rs`, `servico.rs`, `real.rs`, `conexao.rs`, `lib.rs`, teste de reconexão),
`docs/06-linux.md` §7.1.

**Verificação:** 807 testes, clippy silencioso, `xtask` nos três critérios; RPM construído e
exercitado na bancada como na tabela.
