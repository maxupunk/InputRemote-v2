#!/usr/bin/env bash
# Clippy, testes e binarios de bancada **no Linux**, dentro do container do Fedora.
#
# Existe porque esta maquina de desenvolvimento e Windows, e o codigo que so existe no Linux -- o
# backend de clipboard do Wayland, a credencial por SO_PEERCRED, a conferencia pelo descritor em
# /proc -- simplesmente nao compila aqui. Sem este passo, um erro nele so apareceria no meio do
# rpmbuild, ou pior, na maquina de quem instalou.
#
#   /fonte  o repositorio, somente leitura
#   /saida  onde os binarios de bancada saem
#   /alvo   o target/ do Linux, num volume, para a segunda vez nao recompilar tudo

set -euo pipefail

echo "==> copiando a fonte"
rm -rf /tmp/fonte
mkdir -p /tmp/fonte
tar -C /fonte \
    --exclude=./target \
    --exclude=./dist \
    --exclude=./.git \
    --exclude=./empacotar/ferramentas \
    --exclude=./empacotar/certificado \
    --exclude=./spikes \
    -cf - . | tar -C /tmp/fonte -xmf -
# `-m` carimba a hora atual em cada arquivo. Sem ele, o `tar` preserva a data vinda do Windows, e o
# `cargo` -- que decide o que recompilar por data -- podia julgar uma edicao mais velha que o
# artefato em cache no volume. Aconteceu: a bancada recebeu um binario sem a mudanca que estava
# sendo testada. Recompilar o workspace inteiro custa um minuto; testar o binario errado, uma tarde.
cd /tmp/fonte
export CARGO_TARGET_DIR=/alvo

echo "==> clippy"
# A interface fica de fora: ela nao tem codigo so de Linux, e o Slint e a maior parte do tempo.
cargo clippy --workspace --all-targets --locked --exclude ir-ui 2>&1 | tee /tmp/clippy.txt
if grep -qE '^(warning|error)' /tmp/clippy.txt; then
    echo "!!! o clippy tem o que dizer no Linux"
    exit 1
fi

echo "==> testes"
cargo test --workspace --locked --exclude ir-ui

echo "==> binarios de bancada"
cargo build --release --locked --bin inputremote-daemon --bin inputremote-agent
cargo build --release --locked -p ir-ipc --example controle
mkdir -p /saida/bancada-linux
cp /alvo/release/inputremote-daemon /alvo/release/inputremote-agent /saida/bancada-linux/
cp /alvo/release/examples/controle /saida/bancada-linux/
ls -la /saida/bancada-linux/
echo "==> tudo verde no Linux"
