#!/bin/sh
# Download the native installer. No checkout, Rust, Python, or Node is required.
set -eu
version=${XMD_VERSION:-latest}
github_auth=false
next_release=false
for arg in "$@"; do
  if [ "$next_release" = true ]; then version=$arg; next_release=false; continue; fi
  case "$arg" in
    --github-auth) github_auth=true ;;
    --release) next_release=true ;;
    --release=*) version=${arg#--release=} ;;
    --help|-h) echo 'usage: install.sh [--editor zed|vscode] [--github-auth] [--release VERSION] [--bin-dir DIR]'; exit 0 ;;
  esac
done
[ "$next_release" = false ] || { echo 'missing release version' >&2; exit 1; }
platform="$(uname -s)-$(uname -m)"
case "$platform" in
  Darwin-arm64|Darwin-x86_64|Linux-x86_64|Linux-aarch64) ;;
  *) echo "xmd: no prebuilt installer for $platform" >&2; exit 1 ;;
esac
if [ "$version" = latest ]; then
  if [ "$github_auth" = true ]; then
    version=$(gh api repos/drbh/xmd/releases/latest --jq .tag_name)
  else
    url=$(curl -fsSL -o /dev/null -w '%{url_effective}' https://github.com/drbh/xmd/releases/latest)
    version=${url##*/}
  fi
fi
version=${version#v}
case "$version" in
  ''|*[!0-9A-Za-z.+-]*) echo 'invalid release version' >&2; exit 1 ;;
esac
cache=${XMD_INSTALLER_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/xmd/installer}
mkdir -p "$cache/v$version/$platform"
tmp=$(mktemp -d "$cache/v$version/$platform/.download.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT
name="xmd-installer-$platform"
download() {
  asset=$1
  if [ "$github_auth" = true ]; then
    gh release download "v$version" --repo drbh/xmd --pattern "$asset" --dir "$tmp"
  else
    curl -fsSL "https://github.com/drbh/xmd/releases/download/v$version/$asset" -o "$tmp/$asset"
  fi
}
checksum() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}
# Refresh the small checksum so replaced release assets invalidate the cache.
download "$name.sha256"
expected=$(cut -d ' ' -f 1 < "$tmp/$name.sha256")
case "$expected" in
  ''|*[!0-9a-f]*) echo 'xmd: installer checksum mismatch (invalid checksum)' >&2; exit 1 ;;
esac
[ "${#expected}" -eq 64 ] || { echo 'xmd: installer checksum mismatch (invalid checksum)' >&2; exit 1; }
installer="$cache/v$version/$platform/$expected"
if [ ! -f "$installer" ] || [ "$(checksum "$installer")" != "$expected" ]; then
  download "$name"
  [ "$(checksum "$tmp/$name")" = "$expected" ] || { echo 'xmd: installer checksum mismatch' >&2; exit 1; }
  chmod 755 "$tmp/$name"
  # Staging on the same filesystem makes concurrent runs and interruptions safe.
  mv -f "$tmp/$name" "$installer"
fi
XMD_VERSION="$version" "$installer" "$@"
