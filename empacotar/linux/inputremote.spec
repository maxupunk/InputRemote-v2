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

# O grupo `inputremote` e criado na instalacao: e ele que alcanca o canal de controle do servico,
# e sem ele a janela do usuario nao conversa com o servico (docs/02-arquitetura.md, secao 7).
Requires(pre):  shadow-utils

%description
O ponteiro atravessa a borda da tela e passa a controlar o outro computador. Um teclado e um
mouse servem os dois.

Este pacote traz a interface de configuracao e o servico privilegiado, que e quem injeta teclado
e mouse por /dev/uinput -- o caminho que funciona tambem no greeter, na tela de bloqueio e no
console.

No Linux nao ha agente de sessao: a injecao por uinput entra abaixo do compositor, e o proprio
servico a faz. O agente existe so no Windows, onde um servico na sessao 0 nao alcanca a area de
trabalho do usuario.

O servico nao sobe sozinho depois de instalado. Habilite com:

    sudo systemctl enable --now inputremote

%pre
# Um grupo de sistema, sem usuario nenhum dentro. Quem for operar a maquina entra nele de
# proposito -- e isso e uma decisao registrada do administrador, nao permissao frouxa.
getent group inputremote >/dev/null || groupadd -r inputremote
exit 0

%prep
%autosetup -n %{name}-%{version}

%build
export CARGO_NET_OFFLINE=false
# A interface e o servico. O agente nao entra: no Linux quem injeta e o proprio servico, por
# uinput (docs/06-linux.md, secao 2).
cargo build --release --locked --bin inputremote-ui --bin inputremote-daemon

%install
install -Dpm 0755 target/release/inputremote-ui %{buildroot}%{_bindir}/inputremote-ui
install -Dpm 0755 target/release/inputremote-daemon %{buildroot}%{_bindir}/inputremote-daemon
install -Dpm 0644 empacotar/linux/inputremote.desktop \
        %{buildroot}%{_datadir}/applications/%{name}.desktop

# Caminho escrito por extenso, e nao por `%{_unitdir}`: a macro vem de `systemd-rpm-macros`, e
# depender dela so para saber uma pasta fixa acrescentaria um BuildRequires por nada.
install -Dpm 0644 empacotar/linux/inputremote.service \
        %{buildroot}%{_prefix}/lib/systemd/system/%{name}.service

# Um arquivo por tamanho, no lugar que o tema de icones procura. Um PNG grande sozinho obrigaria
# cada lancador a reduzir por conta propria, e cada um reduz de um jeito.
for tamanho in 16 22 24 32 48 64 128 256; do
    install -Dpm 0644 "recursos/icone-${tamanho}.png" \
        "%{buildroot}%{_datadir}/icons/hicolor/${tamanho}x${tamanho}/apps/%{name}.png"
done

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{name}.desktop

%files
%license LICENSE
%doc README.md PROGRESSO.md
%{_bindir}/inputremote-ui
%{_bindir}/inputremote-daemon
%{_prefix}/lib/systemd/system/%{name}.service
%{_datadir}/applications/%{name}.desktop
%{_datadir}/icons/hicolor/*/apps/%{name}.png

%changelog
* Thu Sep 11 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- O servico entra no pacote, com unidade systemd. A interface deixa de ser so demonstracao.

* Thu Sep 10 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- Primeiro pacote: apenas a interface, em modo de demonstracao.
