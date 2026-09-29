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

from a checkout, `cargo install --path cli` builds the same binary; with
nix, `nix run github:drbh/xmd` or `nix build`.
editors: [vs code](client/ide/vscode), [zed](client/ide/zed),
[neovim](client/ide/neovim), [helix](client/ide/helix)

rename a `.md` file to `.x.md` and xmd picks it up; editors without xmd
still show it as markdown

## Features

- work in your preferred editor (lsp first)
- cli tool; query docs via command line
- web-based document editor; edit anywhere anytime - local first (offline capable)
- mutable; written in its own language so you can extend and customize it easily

## docs

short, and in order. each page links to the next

1. [the language](docs/01-language.md)
2. [ask a note a question](docs/02-queries.md)
3. [notes and libraries](docs/03-files.md)
4. [modules](docs/04-modules.md)
5. [put a note on your own page](docs/05-embed.md)
6. [when not to use it](docs/06-limits.md)

## license

MIT, see [LICENSE](LICENSE).
