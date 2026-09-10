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
Release:        0.1.dev%{?dist}
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

%description
O ponteiro atravessa a borda da tela e passa a controlar o outro computador. Um teclado e um
mouse servem os dois.

Este pacote traz apenas a interface de configuracao. O servico privilegiado e o agente de
sessao, que sao o que de fato injeta teclado e mouse, ainda nao foram implementados: sem eles a
interface abre em modo de demonstracao, contra um servico simulado, e avisa isso na propria
janela. Nada e digitado em computador nenhum.

%prep
%autosetup -n %{name}-%{version}

%build
export CARGO_NET_OFFLINE=false
cargo build --release --locked --bin inputremote-ui

%install
install -Dpm 0755 target/release/inputremote-ui %{buildroot}%{_bindir}/inputremote-ui
install -Dpm 0644 empacotar/linux/inputremote.desktop \
        %{buildroot}%{_datadir}/applications/%{name}.desktop

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{name}.desktop || :

%files
%license LICENSE
%doc README.md PROGRESSO.md
%{_bindir}/inputremote-ui
%{_datadir}/applications/%{name}.desktop

%changelog
* Thu Sep 10 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- Primeiro pacote: apenas a interface, em modo de demonstracao.
