# helix

xmd for helix. needs `xmd` on your PATH, see the
[install](../../../README.md) in the main readme

```bash
mkdir -p xmd-helix && \
  curl -fsSL https://github.com/drbh/xmd/releases/latest/download/xmd-helix.tar.gz | \
  tar -xz -C xmd-helix && \
  mkdir -p ~/.config/helix && \
  cp -R xmd-helix/runtime ~/.config/helix/ && \
  cat xmd-helix/languages.toml >> ~/.config/helix/languages.toml
```

to update, copy `runtime` again; `languages.toml` only needs appending once
