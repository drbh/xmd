# neovim

xmd for neovim 0.11+. needs `xmd` on your PATH, see the
[install](../../../README.md) in the main readme

```bash
curl -fsSL https://raw.githubusercontent.com/drbh/xmd/main/client/ide/neovim/xmd.lua \
    -o ~/.config/nvim/xmd.lua && \
  echo "dofile(vim.fn.stdpath('config') .. '/xmd.lua')" \
    >> ~/.config/nvim/init.lua
```
