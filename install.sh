#!/bin/sh
# Installs the xmd binary for this machine from a GitHub release.
#
#   curl -fsSL https://github.com/drbh/xmd/releases/latest/download/install.sh | sh
#
# It downloads the release archive named for `uname -s` and `uname -m`,
# checks it against the release's sha256, and copies the one `xmd` binary
# into ~/.local/bin. Nothing else changes: no sudo, no shell profile edits.
# Run it again to update; `rm ~/.local/bin/xmd` uninstalls.
#
#   XMD_VERSION=0.2.0   install that release instead of the latest
#   XMD_BIN_DIR=dir     install into dir instead of ~/.local/bin
set -eu

version=${XMD_VERSION:-latest}
bin=${XMD_BIN_DIR:-$HOME/.local/bin}

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64 | Darwin-x86_64 | Linux-x86_64 | Linux-aarch64) ;;
  *) echo "xmd: no prebuilt binary for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac
name="xmd-$(uname -s)-$(uname -m).tar.gz"
if [ "$version" = latest ]; then
  url="https://github.com/drbh/xmd/releases/latest/download/$name"
else
  url="https://github.com/drbh/xmd/releases/download/v${version#v}/$name"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "downloading $url"
curl -fsSL "$url" -o "$tmp/$name"
curl -fsSL "$url.sha256" -o "$tmp/$name.sha256"

expected=$(cut -d ' ' -f 1 < "$tmp/$name.sha256")
if command -v sha256sum > /dev/null; then
  actual=$(sha256sum "$tmp/$name" | cut -d ' ' -f 1)
else
  actual=$(shasum -a 256 "$tmp/$name" | cut -d ' ' -f 1)
fi
if [ "$expected" != "$actual" ]; then
  echo "xmd: checksum mismatch for $name (expected $expected, got $actual)" >&2
  exit 1
fi

tar -xzf "$tmp/$name" -C "$tmp" xmd
mkdir -p "$bin"
cp "$tmp/xmd" "$bin/xmd"
chmod 755 "$bin/xmd"
echo "installed $("$bin/xmd" --version) to $bin/xmd"

case ":$PATH:" in
  *":$bin:"*) ;;
  *) echo "$bin is not on your PATH; add this line to your shell profile:"
     echo "  export PATH=\"$bin:\$PATH\"" ;;
esac
