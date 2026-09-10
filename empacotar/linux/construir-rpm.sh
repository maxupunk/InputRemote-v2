#!/usr/bin/env bash
# Monta o RPM. Roda **dentro** do container do Fedora, nunca no Windows.
#
#   /fonte  o repositorio, somente leitura
#   /saida  onde o .rpm sai
#
# Somente leitura de proposito: o empacotamento nao pode sujar a arvore de quem o chamou, e um
# `target/` de Linux escrito por cima do de Windows seria uma tarde perdida.

set -euo pipefail

versao="0.1.0"
pasta="${NOME_DA_PASTA:-inputremote-${versao}}"
espelho="/tmp/fonte/${pasta}"

echo "==> preparando a arvore de rpmbuild"
rpmdev-setuptree

echo "==> copiando a fonte (sem target, dist, .git nem ferramentas)"
rm -rf /tmp/fonte
mkdir -p "${espelho}"
tar -C /fonte \
    --exclude=./target \
    --exclude=./dist \
    --exclude=./.git \
    --exclude=./empacotar/ferramentas \
    --exclude=./empacotar/certificado \
    --exclude=./spikes \
    -cf - . | tar -C "${espelho}" -xf -

echo "==> montando o tarball de origem"
tar -C /tmp/fonte -czf "$HOME/rpmbuild/SOURCES/${pasta}.tar.gz" "${pasta}"

cp "/fonte/empacotar/linux/inputremote.spec" "$HOME/rpmbuild/SPECS/"

echo "==> rpmbuild -bb"
rpmbuild -bb "$HOME/rpmbuild/SPECS/inputremote.spec"

echo "==> copiando o resultado para /saida"
mkdir -p /saida
cp "$HOME"/rpmbuild/RPMS/x86_64/*.rpm /saida/

echo "==> conferindo o pacote"
for arquivo in /saida/*.rpm; do
    echo "--- ${arquivo}"
    rpm -qip "${arquivo}"
    echo "--- conteudo"
    rpm -qlp "${arquivo}"
    echo "--- dependencias de execucao"
    rpm -qRp "${arquivo}"
done
