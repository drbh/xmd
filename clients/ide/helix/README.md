# helix

requires [`xmd` on your PATH](../../../README.md). from a source checkout, run at the repository root:

```bash
make helix
mkdir -p ~/.config/helix
tar -xzf dist/xmd-helix.tar.gz -C ~/.config/helix runtime
```

merge `clients/ide/helix/languages.toml` into `~/.config/helix/languages.toml` once

or [download xmd-helix.tar.gz](https://github.com/drbh/xmd/releases/latest/download/xmd-helix.tar.gz), extract it, copy `runtime` into `~/.config/helix`, and merge its `languages.toml` into your own
