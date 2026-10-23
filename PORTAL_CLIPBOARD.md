# Relatório Técnico: Implementação do Clipboard via XDG Desktop Portal no Linux (GNOME)

> **Documento de Continuidade e Especificação Técnica**  
> **Data:** 23 de Outubro de 2026  
> **Status:** Diagnóstico concluído, spike validado em bancada real (Fedora 44 / GNOME 50), pronto para implementação em Rust (`ir-clip`).

---

## 1. Contexto e Problema Reportado

### 1.1. Sintomas Observados
No Linux (especificamente GNOME Wayland), os seguintes problemas foram identificados durante o uso do InputRemote:
1. **Acúmulo de notificações:** Múltiplas notificações com o título `"InputRemote"` (na tela de bloqueio e na central) ou `"InputRemote está pronto"`.
2. **Ícones duplicados no Dock:** No Dash to Dock / dash do GNOME, acumulam-se dezenas de ícones genéricos do InputRemote (na captura enviada pelo usuário, 11 ícones abertos simultaneamente).
3. **Comportamento em cascata:** O problema se agrava a cada travessia de mouse entre os computadores e a cada cópia/colagem de texto ou arquivos.

---

## 2. Diagnóstico e Causa Raiz

### 2.1. Como o `ir-clip` operava no Linux
O backend do Linux em `crates/ir-clip/src/linux.rs` utiliza o utilitário de linha de comando `wl-clipboard` (`wl-copy` e `wl-paste`):
- Para **ler** (`Clipboard::ler`): executa `timeout 3 wl-paste` para consultar `--list-types`, `text/uri-list`, `text/plain` e `image/png`.
- Para **publicar** (`Clipboard::publicar`): executa `wl-copy --type <tipo>` com entrada via `stdin`, mantendo o processo `wl-copy` em execução em background (`self.donos.push(filho)`), pois no Wayland quem publica deve permanecer vivo para servir o conteúdo a quem colar.
- Para **vigiar** (`vigiar`): tenta `wl-paste --watch`, mas falha no GNOME porque exige o protocolo `data-control`.

### 2.2. A Peculiaridade do GNOME (Mutter)
1. **Ausência de `data-control`:** Ao contrário de compositores baseados em wlroots (Sway) e do KDE Plasma, o GNOME (Mutter) **não implementa** os protocolos `wlr-data-control` nem `ext-data-control` por razões de segurança (isolamento contra espionagem de clipboard em segundo plano).
2. **O workaround do `wl-clipboard`:** Quando o `wl-copy` ou `wl-paste` é executado sem `data-control` fora de foco, ele cria uma superfície gráfica invisível (janela mínima Wayland) para requisitar foco de entrada.
3. **Mutter Focus-Stealing Prevention:** Como o `inputremote-agent --clipboard` roda em segundo plano (via unidade systemd de usuário), o compositor GNOME detecta um aplicativo em background tentando roubar o foco. A reação nativa do GNOME é:
   - Bloquear o foco imediato.
   - Disparar uma notificação do sistema: *"InputRemote está pronto"* (ou apenas *"InputRemote"* na tela de bloqueio).
   - O *Dash to Dock* registra a nova superfície e fixa um ícone temporário no dock.
4. **Persistência de instâncias:** Como o `ir-clip` mantém instâncias do `wl-copy` vivas intencionalmente para segurar a seleção, cada `wl-copy` que gerou uma superfície sem foco fica com um ícone retido no dock. A cada cópia e a cada travessia de mouse (onde `oferecer()` faz 2 a 3 chamadas de `wl-paste`), novos processos e notificações eram empilhados.

---

## 3. Decisão de Solução

Foram levantadas duas alternativas:
- **Opção A:** Utilizar o clipboard do X11 via XWayland (`xclip`).
- **Opção B:** Utilizar o **XDG Desktop Portal** (`org.freedesktop.portal.RemoteDesktop` + `org.freedesktop.portal.Clipboard`).

**Opção escolhida pelo usuário:** **Opção B (Portal)**.  
O Portal é a interface oficial do ecossistema Freedesktop/GNOME para acesso seguro ao clipboard por ferramentas de controle remoto, eliminando completamente a dependência de superfícies Wayland falsas e do `wl-clipboard`.

---

## 4. Ambiente de Teste e Validação em Bancada

### 4.1. Dados da Máquina de Teste (Bancada Remota)
- **IP:** `10.0.0.135`
- **Usuário:** `maxuel`
- **Autenticação:** Senha fornecida e autorizada na sessão.
- **Hostkey SSH:** `SHA256:64ZQt584G5xThfY22AhBplZRFeBORkd91EYQNYGSB5s`
- **Sistema Operacional:** Fedora Linux 44 (Rawhide/Branch 44), Kernel `7.2.11-200.fc44.x86_64`
- **Ambiente Gráfico:** GNOME Shell 50.6 (`XDG_CURRENT_DESKTOP=GNOME`, Wayland `wayland-0`, `DISPLAY=:0`)
- **Pacotes Portal:**
  - `xdg-desktop-portal-1.22.1-1.fc44.x86_64`
  - `xdg-desktop-portal-gnome-50.0-1.fc44.x86_64`

### 4.2. Introspecção D-Bus do Portal
No barramento de sessão (`/run/user/1000/bus`), o serviço `org.freedesktop.portal.Desktop` no caminho `/org/freedesktop/portal/desktop` disponibiliza:

1. **`org.freedesktop.portal.RemoteDesktop`** (versão 2):
   - Propriedade `AvailableDeviceTypes`: `7` (bits 1=Teclado, 2=Mouse, 4=Touchscreen).
   - Método `CreateSession(a{sv} options) -> (o request_handle)`
   - Método `SelectDevices(o session_handle, a{sv} options) -> (o request_handle)`
   - Método `Start(o session_handle, s parent_window, a{sv} options) -> (o request_handle)`
2. **`org.freedesktop.portal.Clipboard`** (versão 1):
   - Método `RequestClipboard(o session_handle, a{sv} options) -> ()`
   - Método `SetSelection(o session_handle, a{sv} options) -> ()`
   - Método `SelectionRead(o session_handle, s mime_type) -> (h fd)`
   - Método `SelectionWrite(o session_handle, u serial) -> (h fd)`
   - Método `SelectionWriteDone(o session_handle, u serial, b success) -> ()`
   - Sinal `SelectionOwnerChanged(o session_handle, a{sv} options)`:
     - `options["mime_types"]`: array de strings (`as`) com os tipos disponíveis no clipboard.
     - `options["session_is_owner"]`: boolean (`b`) indicando se fomos nós quem colocamos.
   - Sinal `SelectionTransfer(o session_handle, s mime_type, u serial)`:
     - Disparado pelo compositor quando uma aplicação externa solicita colar os dados que anunciamos no `SetSelection`.

---

## 5. Resultados do Spike Prático (`spike_portal.py`)

Foi desenvolvido e executado um script de spike em Python (`spike_portal.py`) via systemd user unit (`systemd-run --user`) na máquina de teste para simular o ciclo de vida do portal:

### 5.1. Comportamento sob Tela Bloqueada / Ociosa
- Quando a tela estava em proteção/bloqueio ou ociosa, a chamada a `SelectDevices` falhou com:
  `GDBus.Error:org.freedesktop.DBus.Error.Failed: Session creation inhibited`.
- **Regra de Projeto:** O helper só deve iniciar a sessão do portal quando a sessão do usuário estiver ativa e desbloqueada (ou tratar graciosamente a inibição e tentar quando a sessão for desbloqueada).

### 5.2. Comportamento sob Sessão Ativa e Autorizada
- Com o usuário presente e autorizando o diálogo de acesso remoto do GNOME:
  1. `CreateSession` -> retornou `code: 0`, gerou `session_handle: /org/freedesktop/portal/desktop/session/1_263/irsir1102052_1`.
  2. `SelectDevices(types=0, persist_mode=2)` -> retornou `code: 0`.
     - *Nota:* `types: 0` solicita nenhum dispositivo de entrada (pois o agente de entrada já injeta via `uinput` como root), requisitando apenas permissão de clipboard.
  3. `RequestClipboard(session_handle, {})` -> sucesso.
  4. `Start(session_handle, "", {})` -> completou com `code: 0` após 7.2s de interação do usuário.
     - Retorno do portal: `{'devices': 0, 'clipboard_enabled': True}`.
  5. `SetSelection` com `['text/plain;charset=utf-8', 'text/plain']` -> sucesso imediato.
  6. **Sinais recebidos com sucesso:**
     - `SelectionOwnerChanged`: `{ 'mime_types': ['text/plain;charset=utf-8', 'text/plain'], 'session_is_owner': True }`
     - `SelectionTransfer`: disparado com `mime_type: text/plain;charset=utf-8, serial: 1`.
     - `SelectionWrite` + escrita no File Descriptor + `SelectionWriteDone(success=True)` validado.
  7. **Impacto no desktop:** **Zero notificações indevidas, zero janelas ou ícones duplicados no Dock!**

### 5.3. Análise de Persistência no GNOME (`remotedesktop.c`)
Analisando o código-fonte de `xdg-desktop-portal-gnome` (versão 50.0):
- A estrutura de restauração utiliza `persist_mode`:
  - `0`: Nenhuma persistência.
  - `1`: Persistência transitória.
  - `2`: Persistência permanente (`REMOTE_DESKTOP_PERSIST_MODE_PERSISTENT`).
- No retorno de `Start` ou em `restore_data`, o GNOME empacota `(suv)` ("GNOME", versão 1, `(xxub...)`).
- Ao passar `restore_token` / `restore_data` nas opções de `SelectDevices`, o GNOME restaura a sessão previamente autorizada sem reabrir a janela de diálogo para o usuário.

---

## 6. Arquivos e Scripts de Teste Criados na Sessão

Os seguintes scripts foram criados durante o diagnóstico e testes remotos:

### 6.1. No Windows (Host de Desenvolvimento)
- `C:\Users\Maxuel\.gemini\antigravity\brain\061090e9-dea6-4994-9dd2-f80f6b7d56fb\scratch\nb.ps1`:
  Executa comandos remotos via `plink` com a chave do host do notebook pré-configurada.
- `C:\Users\Maxuel\.gemini\antigravity\brain\061090e9-dea6-4994-9dd2-f80f6b7d56fb\scratch\push.ps1`:
  Transfere arquivos locais para o notebook via `pscp`.
- `C:\Users\Maxuel\.gemini\antigravity\brain\061090e9-dea6-4994-9dd2-f80f6b7d56fb\scratch\probe.sh`:
  Inspeção rápida de variáveis de ambiente, interfaces de D-Bus e logs do systemd.
- `C:\Users\Maxuel\.gemini\antigravity\brain\061090e9-dea6-4994-9dd2-f80f6b7d56fb\scratch\spike_portal.py`:
  Implementação em Python com `Gio.DBusConnection` para handshake completo do RemoteDesktop e Clipboard.
- `C:\Users\Maxuel\.gemini\antigravity\brain\061090e9-dea6-4994-9dd2-f80f6b7d56fb\scratch\spike.sh`:
  Wrapper para rodar o spike isolado dentro do systemd user do Fedora (`systemd-run --user`).
- `C:\Users\Maxuel\.gemini\antigravity\brain\061090e9-dea6-4994-9dd2-f80f6b7d56fb\scratch\remotedesktop.c`:
  Cópia do código-fonte upstream do `xdg-desktop-portal-gnome` 50.0 para referência dos tipos de variantes.

---

## 7. Arquitetura e Plano de Implementação em Rust

### 7.1. Crate Alvo: `crates/ir-clip`
Atualmente `ir-clip` depende de:
- `blake3`, `png`, `tracing`, `thiserror`.
- No Windows: `windows` Win32 OLE / COM.
- No Linux: atualmente apenas `std::process::Command` chamando `wl-clipboard`.

### 7.2. Dependências
O crate `ir-recado` já utiliza `zbus = { version = "5", default-features = false, features = ["async-io", "blocking-api"] }` para Linux (compilado via Cargo lockfile com `zbus 5.14.0`).
- Adicionar `zbus` nas dependências de `crates/ir-clip/Cargo.toml` para `cfg(target_os = "linux")`:
```toml
[target.'cfg(target_os = "linux")'.dependencies]
zbus = { version = "5", default-features = false, features = ["async-io", "blocking-api"] }
```

### 7.3. Estrutura Proposta no `ir-clip`
Substituir o backend puramente baseado em `wl-clipboard` por um módulo flexível:
- `crates/ir-clip/src/linux/`:
  - `mod.rs`: Ponto de entrada (`abrir()`, `vigiar()`).
    - Detecta se há `wlr-data-control` nativo (KDE Plasma, Sway). Se sim, pode usar `wl-clipboard` de forma limpa sem superfícies invisíveis.
    - Se for GNOME ou se `data-control` não existir: inicializa o `PortalClipboard`.
  - `portal.rs`: Implementação do backend D-Bus via `zbus::blocking::Connection`.
    - Mantém a sessão do Portal (`RemoteDesktop` + `Clipboard`).
    - Armazena em cache o `restore_token` em `~/.config/inputremote/portal-token` ou `~/.cache/inputremote/portal-token` para evitar que o diálogo do GNOME seja exibido a cada boot/reconexão.
    - Implementa o trait `Clipboard`:
      - `ler(&mut self) -> Result<Option<Conteudo>>`: Quando o par pede para ler (ou na travessia), se não somos os donos, chama `SelectionRead(mime)` para o tipo apropriado (`text/uri-list`, `text/plain`, `image/png`), lê os bytes do UNIX FD retornado e monta o `Conteudo`.
      - `publicar(&mut self, conteudo: &Conteudo) -> Result<()>`: Guarda o `Conteudo` em memória e chama `SetSelection` informando os tipos MIME disponíveis. Quando chegar o sinal `SelectionTransfer`, atende fornecendo o descritor via `SelectionWrite` e finaliza com `SelectionWriteDone`.
    - Implementa o trait `Vigia`:
      - O sinal `SelectionOwnerChanged(session, options)` do Portal substitui o polling e o `wl-paste --watch`. Ele avisa instantaneamente quando qualquer app copiou algo!

### 7.4. Pacote Linux (`empacotar/linux/`)
- No arquivo `empacotar/linux/inputremote.spec`:
  - `Requires: xdg-desktop-portal` (já é padrão na maioria das distros modernas com GNOME/KDE).
  - Pode manter `wl-clipboard` para sessões wlroots/Sway, mas a dependência estrita para GNOME deixa de causar poluição visual.

---

## 8. Guia Passo a Passo para o Próximo Agente

1. **Adicionar `zbus` em `crates/ir-clip/Cargo.toml`** sob `[target.'cfg(target_os = "linux")'.dependencies]`.
2. **Implementar a conexão e proxies D-Bus** para as interfaces:
   - `org.freedesktop.portal.Desktop`
   - Métodos `CreateSession`, `SelectDevices`, `Start`, `RequestClipboard`, `SetSelection`, `SelectionRead`, `SelectionWrite`, `SelectionWriteDone`.
3. **Mapeamento de tipos MIME:**
   - Texto: `text/plain;charset=utf-8`, `text/plain`, `UTF8_STRING`.
   - Arquivos: `text/uri-list`.
   - Imagens: `image/png`.
4. **Gerenciamento de FDs UNIX:**
   - Usar `std::os::fd::AsRawFd` / `OwnedFd` ou `zbus::zvariant::Fd` para ler e responder nas chamadas de I/O do portal.
5. **Testes e Validação:**
   - Rodar `cargo check --target x86_64-unknown-linux-gnu` ou build no Linux.
   - Enviar binário atualizado para o notebook de teste (`10.0.0.135`) e verificar ausência completa de popups do `wl-clipboard`.
6. **Limpeza de Arquivos Temporários:**
   - Ao finalizar os testes, remover os scripts e arquivos de log em `/tmp/` no notebook e em `scratch/` no Windows conforme a diretriz de `CLEAN UP`.


---

## 9. Anexo: Código Completo dos Scripts de Teste e Validação

### 9.1. Script de Spike em Python (`spike_portal.py`)
```python
#!/usr/bin/env python3
"""Spike: portal RemoteDesktop + Clipboard no GNOME. Mede o que o Rust vai precisar."""
import os, sys, time, json
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

BUS = Gio.bus_get_sync(Gio.BusType.SESSION, None)
NOME = "org.freedesktop.portal.Desktop"
OBJ = "/org/freedesktop/portal/desktop"
SENDER = BUS.get_unique_name()[1:].replace(".", "_")
TOKEN_FILE = os.path.expanduser("~/.cache/ir-spike-token")
DEVICES = int(sys.argv[1]) if len(sys.argv) > 1 else 0
loop = GLib.MainLoop()
contador = [0]


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def chamar(iface, metodo, params, tipo_ret=None, fds=None):
    if fds is not None or (tipo_ret and "h" in tipo_ret):
        ret, fdl = BUS.call_with_unix_fd_list_sync(NOME, OBJ, iface, metodo, params,
                                                   GLib.VariantType(tipo_ret) if tipo_ret else None,
                                                   0, -1, fds, None)
        return ret, fdl
    return BUS.call_sync(NOME, OBJ, iface, metodo, params,
                         GLib.VariantType(tipo_ret) if tipo_ret else None, 0, -1, None)


def pedido(iface, metodo, montar):
    """Chama um método que devolve Request e espera a Response."""
    contador[0] += 1
    token = f"ir{os.getpid()}_{contador[0]}"
    caminho = f"/org/freedesktop/portal/desktop/request/{SENDER}/{token}"
    resultado = {}

    def resposta(conn, sender, path, iface_, sinal, params, _):
        codigo, res = params.unpack()
        resultado["codigo"] = codigo
        resultado["res"] = res
        loop.quit()

    sub = BUS.signal_subscribe(NOME, "org.freedesktop.portal.Request", "Response", caminho,
                               None, Gio.DBusSignalFlags.NO_MATCH_RULE, resposta, None)
    chamar(iface, metodo, montar(token), "(o)")
    loop.run()
    BUS.signal_unsubscribe(sub)
    return resultado["codigo"], resultado["res"]


def main():
    try:
        if os.environ.get("NOREG"):
            raise RuntimeError("pulado")
        BUS.call_sync(NOME, OBJ, "org.freedesktop.host.portal.Registry", "Register",
                      GLib.Variant("(sa{sv})", ("inputremote", {})), None, 0, -1, None)
        log("registrado como inputremote")
    except Exception as e:
        log("sem Registry:", e)

    RD = "org.freedesktop.portal.RemoteDesktop"
    CB = "org.freedesktop.portal.Clipboard"
    cod, res = pedido(RD, "CreateSession", lambda t: GLib.Variant("(a{sv})", ({
        "handle_token": GLib.Variant("s", t),
        "session_handle_token": GLib.Variant("s", "irs" + t)},)))
    log("CreateSession", cod, res)
    sessao = res["session_handle"]

    opcoes = {"persist_mode": GLib.Variant("u", 2)}
    if DEVICES >= 0:
        opcoes["types"] = GLib.Variant("u", DEVICES)
    if os.path.exists(TOKEN_FILE):
        opcoes["restore_token"] = GLib.Variant("s", open(TOKEN_FILE).read().strip())
        log("usando restore_token")

    def sel(t):
        o = dict(opcoes)
        o["handle_token"] = GLib.Variant("s", t)
        return GLib.Variant("(oa{sv})", (sessao, o))
    cod, res = pedido(RD, "SelectDevices", sel)
    log("SelectDevices", cod, res)
    if cod != 0:
        return

    chamar(CB, "RequestClipboard", GLib.Variant("(oa{sv})", (sessao, {})))
    log("RequestClipboard ok")

    t0 = time.time()
    cod, res = pedido(RD, "Start", lambda t: GLib.Variant("(osa{sv})", (sessao, "", {
        "handle_token": GLib.Variant("s", t)})))
    log("Start", cod, res, f"{time.time()-t0:.1f}s")
    if cod != 0:
        return
    if "restore_token" in res:
        os.makedirs(os.path.dirname(TOKEN_FILE), exist_ok=True)
        open(TOKEN_FILE, "w").write(res["restore_token"])

    conteudo = {"text/plain;charset=utf-8": b"ola do portal", "text/plain": b"ola do portal"}

    def mudou(conn, sender, path, iface_, sinal, params, _):
        s, op = params.unpack()
        log("SelectionOwnerChanged", op)
        if not op.get("session_is_owner", False) and "text/plain;charset=utf-8" in op.get("mime_types", []):
            try:
                ret, fdl = chamar(CB, "SelectionRead", GLib.Variant("(os)", (sessao, "text/plain;charset=utf-8")), "(h)", Gio.UnixFDList())
                fd = fdl.get(ret.unpack()[0])
                dados = b""
                while True:
                    b = os.read(fd, 65536)
                    if not b:
                        break
                    dados += b
                os.close(fd)
                log("lido:", dados[:80])
            except Exception as e:
                log("SelectionRead falhou:", e)

    def transferir(conn, sender, path, iface_, sinal, params, _):
        s, mime, serial = params.unpack()
        log("SelectionTransfer", mime, serial)
        ok = False
        try:
            ret, fdl = chamar(CB, "SelectionWrite", GLib.Variant("(ou)", (sessao, serial)), "(h)", Gio.UnixFDList())
            fd = fdl.get(ret.unpack()[0])
            os.write(fd, conteudo.get(mime, b""))
            os.close(fd)
            ok = mime in conteudo
        except Exception as e:
            log("SelectionWrite falhou:", e)
        chamar(CB, "SelectionWriteDone", GLib.Variant("(oub)", (sessao, serial, ok)))

    def fechou(*a):
        log("sessao fechada", a[5])
        loop.quit()

    BUS.signal_subscribe(NOME, CB, "SelectionOwnerChanged", OBJ, None, Gio.DBusSignalFlags.NONE, mudou, None)
    BUS.signal_subscribe(NOME, CB, "SelectionTransfer", OBJ, None, Gio.DBusSignalFlags.NONE, transferir, None)
    BUS.signal_subscribe(NOME, "org.freedesktop.portal.Session", "Closed", sessao, None, Gio.DBusSignalFlags.NONE, fechou, None)

    chamar(CB, "SetSelection", GLib.Variant("(oa{sv})", (sessao, {
        "mime_types": GLib.Variant("as", list(conteudo.keys()))})))
    log("SetSelection ok; esperando colagens e mudancas")
    GLib.timeout_add_seconds(int(os.environ.get("DURA", "60")), loop.quit)
    loop.run()
    log("fim")


if __name__ == "__main__":
    main()
```

### 9.2. Script de Execução Isolada (`spike.sh`)
```sh
#!/bin/sh
systemctl --user stop irspike 2>/dev/null
systemctl --user reset-failed irspike 2>/dev/null
systemd-run --user --unit=irspike -E DURA=${DURA:-150} -E NOREG=$2 python3 -u /tmp/spike_portal.py $1 >/dev/null 2>&1
sleep ${ESPERA:-5}
journalctl --user -u irspike --no-pager -o cat --since "-1min" | tail -25
echo ---- portal
journalctl --user --since "-1min" --no-pager -o cat | grep -i -E 'portal|remote|mutter' | grep -v irspike | tail -15
```

### 9.3. Registro da Saída da Validação Positiva
```text
Started irspike.service - [systemd-run] /usr/bin/python3 -u /tmp/spike_portal.py 0.
14:59:10 registrado como inputremote
14:59:10 CreateSession 0 {'session_handle': '/org/freedesktop/portal/desktop/session/1_263/irsir1102052_1'}
14:59:10 SelectDevices 0 {}
14:59:10 RequestClipboard ok
14:59:18 Start 0 {'devices': 0, 'clipboard_enabled': True} 7.2s
14:59:18 SetSelection ok; esperando colagens e mudancas
14:59:18 SelectionOwnerChanged {'mime_types': ['text/plain;charset=utf-8', 'text/plain'], 'session_is_owner': True}
14:59:18 SelectionTransfer text/plain;charset=utf-8 1
```

