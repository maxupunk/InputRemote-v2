# O binario sai com `strip = "symbols"` pelo perfil de release do projeto, entao nao ha simbolos
# para um pacote de depuracao empacotar. Deixar o rpmbuild tentar produz um erro no fim de uma
# compilacao de varios minutos, que e o pior momento para descobrir.
%global debug_package %{nil}
%global _build_id_links none

# A versao de desenvolvimento nao vira `Version`, porque RPM nao aceita `-` ali. Vai para
# `Release` com o prefixo `0.`, que e a convencao de pre-lancamento: assim 0.1.0-0.1.dev ordena
# **antes** de 0.1.0-1, e o dia do lancamento a atualizacao acontece sozinha.
%global versao_do_projeto 0.1.0-dev

Name:           inputremote
Version:        0.1.0
# O carimbo vem de `construir-rpm.sh`, e existe por um motivo concreto: enquanto a versao de
# desenvolvimento nao muda, dois pacotes diferentes teriam a mesma NEVR -- e `dnf install` sobre
# uma NEVR ja instalada nao faz nada, sai com sucesso e deixa o pacote velho no lugar. O sintoma e
# "instalei e continua igual", que e o pior tipo de falha: silenciosa e com cara de sucesso.
Release:        0.1.dev%{?carimbo}%{?dist}
Summary:        Compartilha teclado e mouse entre dois computadores

License:        MIT
URL:            https://github.com/inputremote/inputremote
Source0:        %{name}-%{version}.tar.gz

ExclusiveArch:  x86_64

BuildRequires:  gcc
BuildRequires:  gcc-c++
BuildRequires:  make
BuildRequires:  pkgconf-pkg-config
BuildRequires:  fontconfig-devel
BuildRequires:  freetype-devel
BuildRequires:  libxkbcommon-devel
BuildRequires:  libxkbcommon-x11-devel
BuildRequires:  wayland-devel
BuildRequires:  libX11-devel
BuildRequires:  libXcursor-devel
BuildRequires:  libXrandr-devel
BuildRequires:  libXi-devel
BuildRequires:  mesa-libGL-devel
BuildRequires:  mesa-libEGL-devel
BuildRequires:  desktop-file-utils

# O tema de icones e quem resolve `Icon=inputremote` para um arquivo. Sem ele o lancador mostra o
# icone generico, que e o mesmo que nao ter icone.
Requires:       hicolor-icon-theme

# O clipboard do Wayland, lido e escrito pelo ajudante da sessao. `timeout` vem do coreutils, que
# toda instalacao tem; o prazo e o que impede um compositor mudo de travar o ajudante (ADR-0011).
Requires:       wl-clipboard
Requires:       coreutils

# O grupo `inputremote` e criado na instalacao: e ele que alcanca o canal de controle do servico,
# e sem ele a janela do usuario nao conversa com o servico (docs/02-arquitetura.md, secao 7).
Requires(pre):  shadow-utils

# O pedido de senha da janela: `pkexec` e o dialogo do ambiente grafico mostram a explicacao de
# `io.github.inputremote.ativar` e so entao rodam o ajudante que liga o servico e da acesso a ele.
Requires:       polkit
# `systemctl preset`/`enable` nos roteiros de instalacao, e o `usermod` do ajudante.
Requires(post): systemd
Requires(preun): systemd
Requires(postun): systemd
Requires:       shadow-utils

%description
O ponteiro atravessa a borda da tela e passa a controlar o outro computador. Um teclado e um
mouse servem os dois.

Este pacote traz a interface de configuracao e o servico privilegiado, que e quem injeta teclado
e mouse por /dev/uinput -- o caminho que funciona tambem no greeter, na tela de bloqueio e no
console.

A injecao por uinput entra abaixo do compositor, e o proprio servico a faz. O que roda na sessao
do usuario e so o ajudante de clipboard, iniciado com a sessao: ele leva ao outro computador o que
estiver no clipboard quando o mouse atravessa a borda, e poe no clipboard daqui o que chegar de la.
Ele roda como o usuario e fala pelo mesmo canal que a janela -- entao o usuario precisa estar no
grupo `inputremote`.

O servico e habilitado e iniciado na instalacao. Na primeira vez que a janela abre, ela oferece
"Ativar o InputRemote": o sistema pede a senha de administrador, uma unica vez, com a explicacao
do que vai mudar, e o usuario passa a ter acesso ao servico -- sem terminal e sem reiniciar.

%pre
# Um grupo de sistema, sem usuario nenhum dentro. Quem for operar a maquina entra nele de
# proposito -- e isso e uma decisao registrada do administrador, nao permissao frouxa. A janela
# oferece essa decisao pelo polkit ("Ativar o InputRemote"), e ela so acontece com a senha dele.
getent group inputremote >/dev/null || groupadd -r inputremote
exit 0

%post
# Primeira instalacao: o preset habilita, e o servico ja sobe. Sem isso a janela abria dizendo que
# o servico nao responde, e a primeira experiencia com o produto era um comando de terminal.
# Falhar aqui nao pode falhar a instalacao: a janela oferece ativar de novo.
if [ "$1" -eq 1 ]; then
    systemctl daemon-reload >/dev/null 2>&1 || :
    systemctl preset inputremote.service >/dev/null 2>&1 || :
    systemctl start inputremote.service >/dev/null 2>&1 || :
fi

%preun
# Remocao (e nao atualizacao): para e desabilita antes de os arquivos sumirem. Sem isso o servico
# continuava rodando um binario apagado ate o proximo reinicio.
if [ "$1" -eq 0 ]; then
    systemctl disable --now inputremote.service >/dev/null 2>&1 || :
fi

%postun
systemctl daemon-reload >/dev/null 2>&1 || :
# Atualizacao: o servico volta com o binario novo. So se ja estava rodando -- quem o parou de
# proposito nao o ve subir sozinho por causa de uma atualizacao.
if [ "$1" -ge 1 ]; then
    systemctl try-restart inputremote.service >/dev/null 2>&1 || :
fi

%prep
%autosetup -n %{name}-%{version}

%build
export CARGO_NET_OFFLINE=false
# A interface, o servico e o agente. No Linux o agente nao injeta -- quem injeta e o servico, por
# uinput (docs/06-linux.md, secao 2) --; ele entra no papel de ajudante de clipboard (ADR-0011).
cargo build --release --locked --bin inputremote-ui --bin inputremote-daemon --bin inputremote-agent

%install
install -Dpm 0755 target/release/inputremote-ui %{buildroot}%{_bindir}/inputremote-ui
install -Dpm 0755 target/release/inputremote-daemon %{buildroot}%{_bindir}/inputremote-daemon
install -Dpm 0755 target/release/inputremote-agent %{buildroot}%{_bindir}/inputremote-agent
# Iniciado com a sessao grafica de cada usuario, pelo mecanismo padrao do XDG.
install -Dpm 0644 empacotar/linux/inputremote-clipboard.desktop         %{buildroot}%{_sysconfdir}/xdg/autostart/inputremote-clipboard.desktop
install -Dpm 0644 empacotar/linux/inputremote.desktop \
        %{buildroot}%{_datadir}/applications/%{name}.desktop

# Caminho escrito por extenso, e nao por `%{_unitdir}`: a macro vem de `systemd-rpm-macros`, e
# depender dela so para saber uma pasta fixa acrescentaria um BuildRequires por nada.
install -Dpm 0644 empacotar/linux/inputremote.service \
        %{buildroot}%{_prefix}/lib/systemd/system/%{name}.service

# O ajudante que liga o servico e da acesso a quem pediu, e a politica do polkit que explica o
# pedido de senha. `libexec`, e nao `bin`: nao e um comando para o usuario digitar.
install -Dpm 0755 empacotar/linux/ativar %{buildroot}%{_libexecdir}/%{name}/ativar
install -Dpm 0644 empacotar/linux/io.github.inputremote.ativar.policy \
        %{buildroot}%{_datadir}/polkit-1/actions/io.github.inputremote.ativar.policy
install -Dpm 0644 empacotar/linux/80-inputremote.preset \
        %{buildroot}%{_prefix}/lib/systemd/system-preset/80-%{name}.preset
# As portas do produto como servico do firewalld; o ajudante de ativacao o liga na zona padrao.
install -Dpm 0644 empacotar/linux/inputremote-firewalld.xml \
        %{buildroot}%{_prefix}/lib/firewalld/services/%{name}.xml

# Um arquivo por tamanho, no lugar que o tema de icones procura. Um PNG grande sozinho obrigaria
# cada lancador a reduzir por conta propria, e cada um reduz de um jeito.
for tamanho in 16 22 24 32 48 64 128 256; do
    install -Dpm 0644 "recursos/icone-${tamanho}.png" \
        "%{buildroot}%{_datadir}/icons/hicolor/${tamanho}x${tamanho}/apps/%{name}.png"
done

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{name}.desktop
desktop-file-validate %{buildroot}%{_sysconfdir}/xdg/autostart/inputremote-clipboard.desktop

%files
%license LICENSE
%doc README.md PROGRESSO.md
%{_bindir}/inputremote-ui
%{_bindir}/inputremote-daemon
%{_bindir}/inputremote-agent
%{_prefix}/lib/systemd/system/%{name}.service
%{_prefix}/lib/systemd/system-preset/80-%{name}.preset
%dir %{_libexecdir}/%{name}
%{_libexecdir}/%{name}/ativar
%{_datadir}/polkit-1/actions/io.github.inputremote.ativar.policy
%dir %{_prefix}/lib/firewalld
%dir %{_prefix}/lib/firewalld/services
%{_prefix}/lib/firewalld/services/%{name}.xml
%config(noreplace) %{_sysconfdir}/xdg/autostart/inputremote-clipboard.desktop
%{_datadir}/applications/%{name}.desktop
%{_datadir}/icons/hicolor/*/apps/%{name}.png

%changelog
* Sat Sep 19 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- Parear sem configurar nada: descoberta na rede local (mDNS) e lista dos Bluetooth pareados.
- As portas do produto como servico do firewalld, ligado pelo ajudante de ativacao.

* Fri Sep 18 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- O servico e habilitado e iniciado na instalacao, e parado na remocao.
- A janela pede a senha de administrador pelo polkit, com a explicacao, para ligar o servico e dar
  acesso ao usuario, no lugar de mandar rodar comandos no terminal.

* Fri Sep 18 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- Copiar e colar: o ajudante de clipboard entra, iniciado com a sessao, e arquivos atravessam por TCP.
- ProtectHome=read-only: o servico precisa ler o que o usuario copia da pasta pessoal.

* Fri Sep 11 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- O servico entra no pacote, com unidade systemd. A interface deixa de ser so demonstracao.

* Thu Sep 10 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- Primeiro pacote: apenas a interface, em modo de demonstracao.
