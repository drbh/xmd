# neovim

xmd for neovim 0.11+. needs `xmd` on your PATH, see the
[install](../../../README.md) in the main readme

```bash
curl -fsSL https://github.com/drbh/xmd/releases/latest/download/xmd-neovim.tar.gz | \
    tar -xzC ~/.config/nvim xmd.lua && \
  echo "dofile(vim.fn.stdpath('config') .. '/xmd.lua')" \
    >> ~/.config/nvim/init.lua
```

to update, run the `curl` line again; the `dofile` line only needs adding once
