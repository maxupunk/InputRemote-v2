# Imagem de compilacao para o RPM do Fedora.
#
# Existe para que o RPM saia de um Fedora de verdade, e nao de uma compilacao cruzada. Um pacote
# que nunca viu o sistema de destino descobre as dependencias erradas na maquina de quem instala.
#
# A imagem e construida uma vez e reaproveitada; so o `cargo build` roda a cada empacotamento.
FROM fedora:44

# Dependencias de compilacao do Slint com backend winit/femtovg, mais as ferramentas de RPM.
RUN dnf -y install \
        gcc gcc-c++ make pkgconf-pkg-config git curl-minimal \
        rpm-build rpmdevtools \
        fontconfig-devel freetype-devel \
        libxkbcommon-devel libxkbcommon-x11-devel \
        wayland-devel wayland-protocols-devel \
        libX11-devel libXcursor-devel libXrandr-devel libXi-devel libxcb-devel \
        mesa-libGL-devel mesa-libEGL-devel \
    && dnf clean all

# A versao do compilador e a de rust-toolchain.toml, e nao a que o Fedora empacota. O pacote
# precisa sair do mesmo compilador que o resto do projeto usa; senao "funciona aqui" deixa de
# significar alguma coisa.
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain 1.96.0 \
        --target x86_64-unknown-linux-gnu

WORKDIR /trabalho
