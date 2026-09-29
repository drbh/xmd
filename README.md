# xmd

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/drbh/xmd/releases/download/media/typing-dark.gif">
  <img alt="a finished trip note: one number changes and the total follows, the departure date changes and the countdown and due date follow, then tables, checklists and timers are typed in" src="https://github.com/drbh/xmd/releases/download/media/typing-light.gif" width="720">
</picture>

cli and language server (mac and linux):

```bash
mkdir -p ~/.local/bin && \
  curl -fsSL https://github.com/drbh/xmd/releases/latest/download/xmd-$(uname -s)-$(uname -m).tar.gz | \
  tar -xzC ~/.local/bin xmd
```

editors: [vs code](client/ide/vscode), [zed](client/ide/zed),
[neovim](client/ide/neovim), [helix](client/ide/helix)

now just update your `.md` files to `.x.md` and they will be recognized by xmd - and automatically fallback to markdown rendering by unsupported editors.

## Features

- work in your preferred editor (lsp first)
- cli tool; query docs via command line
- web-based document editor; edit anywhere anytime - local first (offline capable)
- mutable; written in its own language so you can extend and customize it easily

## license

MIT, see [LICENSE](LICENSE).
