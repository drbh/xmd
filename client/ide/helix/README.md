# helix

xmd for helix. needs `xmd` on your PATH, see the
[install](../../../README.md) in the main readme

```bash
mkdir -p ~/.config/helix/runtime/queries/xmd && \
  curl -fsSL https://raw.githubusercontent.com/drbh/xmd/main/client/ide/helix/languages.toml \
    >> ~/.config/helix/languages.toml && \
  for f in highlights injections; do \
    curl -fsSL https://raw.githubusercontent.com/drbh/xmd/main/client/ide/helix/runtime/queries/xmd/$f.scm \
      -o ~/.config/helix/runtime/queries/xmd/$f.scm; \
  done
```
