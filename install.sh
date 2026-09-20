#!/bin/sh
# Installs the wtf binary (the command line and the language server) from the
# GitHub release for this machine. No Rust toolchain needed.
#
#   curl -fsSL https://github.com/drbh/jot/releases/latest/download/install.sh | sh
#
#   WTF_VERSION=v0.1.0     pin a release instead of the latest one
#   WTF_INSTALL_DIR=DIR    install somewhere other than ~/.local/bin
#   WTF_ASSET_URL=URL      fetch the archive and checksums.txt from this
#                          directory URL instead of GitHub (for testing;
#                          file:// works too)
set -eu

repo="drbh/jot"
install_dir="${WTF_INSTALL_DIR:-$HOME/.local/bin}"

fail() {
  echo "wtf: $*" >&2
  exit 1
}

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Darwin) os_target="apple-darwin" ;;
  Linux) os_target="unknown-linux-gnu" ;;
  MINGW* | MSYS* | CYGWIN*) fail "on Windows run: irm https://github.com/$repo/releases/latest/download/install.ps1 | iex" ;;
  *) fail "no prebuilt binary for $os; build one with: cargo install --git https://github.com/$repo wtf" ;;
esac
case "$arch" in
  x86_64 | amd64) arch_target="x86_64" ;;
  arm64 | aarch64) arch_target="aarch64" ;;
  *) fail "no prebuilt binary for $os $arch; build one with: cargo install --git https://github.com/$repo wtf" ;;
esac
target="$arch_target-$os_target"
asset="wtf-$target.tar.gz"

if [ -n "${WTF_ASSET_URL:-}" ]; then
  base="${WTF_ASSET_URL%/}"
elif [ -n "${WTF_VERSION:-}" ]; then
  base="https://github.com/$repo/releases/download/$WTF_VERSION"
else
  base="https://github.com/$repo/releases/latest/download"
fi

fetch() {
  # fetch URL FILE
  if command -v curl > /dev/null 2>&1; then
    curl -fsSL --proto '=https,file' --retry 3 -o "$2" "$1"
  elif command -v wget > /dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    fail "curl or wget is needed to download $1"
  fi
}

sha256() {
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum > /dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  elif command -v openssl > /dev/null 2>&1; then
    openssl dgst -sha256 "$1" | sed 's/.*= //'
  else
    fail "sha256sum, shasum or openssl is needed to verify the download"
  fi
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "downloading $base/$asset"
fetch "$base/$asset" "$tmp/$asset"
fetch "$base/checksums.txt" "$tmp/checksums.txt"

expected=$(grep "  $asset\$" "$tmp/checksums.txt" | head -n1 | cut -d' ' -f1)
[ -n "$expected" ] || fail "$asset is not listed in checksums.txt"
actual=$(sha256 "$tmp/$asset")
[ "$actual" = "$expected" ] || fail "checksum mismatch for $asset: expected $expected, got $actual"

tar -xzf "$tmp/$asset" -C "$tmp" wtf
mkdir -p "$install_dir"
# Replace by rename so a running language server keeps its old executable.
chmod 755 "$tmp/wtf"
mv -f "$tmp/wtf" "$install_dir/wtf"

echo "installed $("$install_dir/wtf" --version) to $install_dir/wtf"
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *)
    echo
    echo "$install_dir is not on your PATH. Add it to your shell profile:"
    echo "  export PATH=\"$install_dir:\$PATH\""
    ;;
esac
