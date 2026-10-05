# neovim

requires Neovim 0.11+ and [`xmd` on your PATH](../../../README.md). from a source checkout, run at the repository root:

```bash
make neovim
mkdir -p ~/.config/nvim
tar -xzf dist/xmd-neovim.tar.gz -C ~/.config/nvim
```

add `dofile(vim.fn.stdpath('config') .. '/xmd.lua')` to `~/.config/nvim/init.lua` once

or [download xmd-neovim.tar.gz](https://github.com/drbh/xmd/releases/latest/download/xmd-neovim.tar.gz) and extract it into `~/.config/nvim`, then add the same line
