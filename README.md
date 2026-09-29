# xmd

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/drbh/jot/releases/download/media/typing-dark.gif">
  <img alt="a finished trip note: one number changes and the total follows, the departure date changes and the countdown and due date follow, then tables, checklists and timers are typed in" src="https://github.com/drbh/jot/releases/download/media/typing-light.gif" width="720">
</picture>

cli and language server (mac and linux):

```bash
mkdir -p ~/.local/bin && curl -fsSL https://github.com/drbh/jot/releases/latest/download/xmd-$(uname -s)-$(uname -m).tar.gz | tar -xzC ~/.local/bin xmd
```

vs code:

```bash
curl -fsSLO https://github.com/drbh/jot/releases/latest/download/xmd.vsix && code --install-extension xmd.vsix
```

zed (then Extensions > Install Dev Extension > `xmd-zed`):

```bash
mkdir -p xmd-zed && curl -fsSL https://github.com/drbh/jot/releases/latest/download/xmd-zed.tar.gz | tar -xz -C xmd-zed
```

neovim 0.11+:

```bash
curl -fsSL https://raw.githubusercontent.com/drbh/jot/main/client/ide/neovim/xmd.lua -o ~/.config/nvim/xmd.lua && echo "dofile(vim.fn.stdpath('config') .. '/xmd.lua')" >> ~/.config/nvim/init.lua
```

helix:

```bash
mkdir -p ~/.config/helix/runtime/queries/xmd && curl -fsSL https://raw.githubusercontent.com/drbh/jot/main/client/ide/helix/languages.toml >> ~/.config/helix/languages.toml && for f in highlights injections; do curl -fsSL https://raw.githubusercontent.com/drbh/jot/main/client/ide/helix/runtime/queries/xmd/$f.scm -o ~/.config/helix/runtime/queries/xmd/$f.scm; done
```

now just update your `.md` files to `.x.md` and they will be recognized by xmd - and automatically fallback to markdown rendering by unsupported editors.

## Features

- work in your preferred editor (lsp first)
- cli tool; query docs via command line
- web-based document editor; edit anywhere anytime - local first (offline capable)
- mutable; written in its own language so you can extend and customize it easily
