# O binario sai com `strip = "symbols"` pelo perfil de release do projeto, entao nao ha simbolos
# para um pacote de depuracao empacotar. Deixar o rpmbuild tentar produz um erro no fim de uma
# compilacao de varios minutos, que e o pior momento para descobrir.
%global debug_package %{nil}
%global _build_id_links none

%global versao_do_projeto 0.1.0

Name:           inputremote
Version:        0.1.0
Release:        1%{?carimbo}%{?dist}
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

# O `notify-send`, que e como o ajudante conta na tela o que esta acontecendo com uma copia: o
# comeco, o fim, e o motivo quando ela nao atravessa. Sem isso copiar e colar era mudo, e colar do
# outro lado trazia a copia anterior sem nenhum sinal de que aquilo era um resto.
Requires:       libnotify
# A economia de energia do Wi-Fi: `iw` le se a placa cochila entre pacotes e a desliga quando o
# usuario clica em Resolver. Sem ele o aviso nao aparece, e o mouse pela rede trava sem motivo
# visivel (docs/logs/44).
Requires:       iw

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
do usuario e so o ajudante de clipboard, uma unidade do systemd do usuario que sobe com a sessao
grafica e volta sempre que sair: ele leva ao outro computador o que
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

%posttrans
# Instalacao ou atualizacao, ja com o pacote velho fora: o ajudante de clipboard sobe (ou volta, com
# o binario novo) nas sessoes graficas abertas. A unidade sozinha so subiria no proximo login --
# e copiar e colar ficaria parado ate la, sem nada na tela dizendo por que.
%{_libexecdir}/%{name}/ajudante-nas-sessoes >/dev/null 2>&1 || :

%preun
# Remocao (e nao atualizacao): para e desabilita antes de os arquivos sumirem. Sem isso o servico
# continuava rodando um binario apagado ate o proximo reinicio.
if [ "$1" -eq 0 ]; then
    %{_libexecdir}/%{name}/ajudante-nas-sessoes parar >/dev/null 2>&1 || :
    systemctl disable --now inputremote.service >/dev/null 2>&1 || :
    # O servico do firewalld sai da zona antes de o arquivo dele sumir; depois nao ha mais como.
    firewall-cmd --permanent --remove-service=%{name} >/dev/null 2>&1 || :
    firewall-cmd --reload >/dev/null 2>&1 || :
fi

%postun
systemctl daemon-reload >/dev/null 2>&1 || :
# Remocao (e nao atualizacao): nada do produto fica para tras. A chave da maquina sai junto -- um
# par que a conhecia nao alcanca mais este computador sem parear de novo. O ajuste do Wi-Fi volta
# ao padrao do sistema.
if [ "$1" -eq 0 ]; then
    rm -rf /var/lib/%{name} >/dev/null 2>&1 || :
    rm -f /etc/NetworkManager/conf.d/90-%{name}-wifi.conf >/dev/null 2>&1 || :
fi
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
# O ajudante de clipboard: unidade do usuario, ligada a sessao grafica por um link que vem no
# pacote -- vale para todo usuario, sem `systemctl --global enable` nem preset.
install -Dpm 0644 empacotar/linux/inputremote-clipboard.service \
        %{buildroot}%{_prefix}/lib/systemd/user/%{name}-clipboard.service
install -d %{buildroot}%{_prefix}/lib/systemd/user/graphical-session.target.wants
ln -s ../%{name}-clipboard.service \
        %{buildroot}%{_prefix}/lib/systemd/user/graphical-session.target.wants/%{name}-clipboard.service
install -Dpm 0755 empacotar/linux/ajudante-nas-sessoes %{buildroot}%{_libexecdir}/%{name}/ajudante-nas-sessoes
install -Dpm 0644 empacotar/linux/inputremote.desktop \
        %{buildroot}%{_datadir}/applications/%{name}.desktop

# Caminho escrito por extenso, e nao por `%{_unitdir}`: a macro vem de `systemd-rpm-macros`, e
# depender dela so para saber uma pasta fixa acrescentaria um BuildRequires por nada.
install -Dpm 0644 empacotar/linux/inputremote.service \
        %{buildroot}%{_prefix}/lib/systemd/system/%{name}.service

# O gancho de suspensao: o servico solta tudo e avisa o par antes de a maquina dormir.
install -Dpm 0755 empacotar/linux/inputremote-sleep \
        %{buildroot}%{_prefix}/lib/systemd/system-sleep/%{name}

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

%files
%license LICENSE
%doc README.md PROGRESSO.md
%{_bindir}/inputremote-ui
%{_bindir}/inputremote-daemon
%{_bindir}/inputremote-agent
%{_prefix}/lib/systemd/system/%{name}.service
%{_prefix}/lib/systemd/system-preset/80-%{name}.preset
%{_prefix}/lib/systemd/system-sleep/%{name}
%dir %{_libexecdir}/%{name}
%{_libexecdir}/%{name}/ativar
%{_libexecdir}/%{name}/ajudante-nas-sessoes
%{_prefix}/lib/systemd/user/%{name}-clipboard.service
%dir %{_prefix}/lib/systemd/user/graphical-session.target.wants
%{_prefix}/lib/systemd/user/graphical-session.target.wants/%{name}-clipboard.service
%{_datadir}/polkit-1/actions/io.github.inputremote.ativar.policy
%dir %{_prefix}/lib/firewalld
%dir %{_prefix}/lib/firewalld/services
%{_prefix}/lib/firewalld/services/%{name}.xml
%{_datadir}/applications/%{name}.desktop
%{_datadir}/icons/hicolor/*/apps/%{name}.png

%changelog
* Fri Sep 25 2026 InputRemote <inputremote@example.invalid> - 0.1.0-1
- Primeiro lancamento oficial (0.1.0).

* Sat Sep 19 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- Copiar e colar deixa de ser mudo: o ajudante conta na notificacao do sistema o que esta sendo
  copiado, quando termina e por que nao atravessou.
- O ajudante de clipboard vira unidade do systemd do usuario, com Restart=always, e o pacote o
  (re)inicia nas sessoes abertas: instalar ou atualizar nao deixa mais copiar e colar parado.

* Sat Sep 19 2026 InputRemote <inputremote@example.invalid> - 0.1.0-0.1.dev
- Parear sem configurar nada: descoberta na rede local e lista dos Bluetooth pareados.
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
