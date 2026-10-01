#!/usr/bin/env bash
# Regenerate theme/fonts/*.woff2 from the Ioskeley Mono CB family (the build
# with checkbox and operator ligatures used in Zed). Keeps every OpenType
# feature so `[ ]`, `[x]`, `:=`, and `->` render as ligatures, and the symbol
# ranges the engine emits in inlays. Requires fonttools with brotli.
set -euo pipefail
cd "$(dirname "$0")/../theme/fonts"
src="${XMD_FONT_SOURCE:-$HOME/Library/Fonts}"
ranges='U+0020-007E,U+00A0-00FF,U+0100-017F,U+2000-203A,U+2044,U+20A0-20BF,U+2190-21FF,U+2212,U+2260-2265,U+2500-259F,U+25A0-25FF,U+2610-2612,U+2713-2718,U+2B05-2B0D'
for style in Regular Bold Italic BoldItalic; do
  pyftsubset "$src/IoskeleyMono-$style.ttf" --output-file="IoskeleyMono-$style.subset.woff2" --flavor=woff2 \
    --unicodes="$ranges" --layout-features=calt,ccmp,locl,kern,mark,mkmk --name-IDs='*' --notdef-outline
done
ls -la
