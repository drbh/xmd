# xmd

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/drbh/xmd/releases/download/media/typing-dark.gif">
  <img alt="a finished trip note: one number changes and the total follows, the departure date changes and the countdown and due date follow, then tables, checklists and timers are typed in" src="https://github.com/drbh/xmd/releases/download/media/typing-light.gif" width="720">
</picture>

cli and language server (mac and linux):

```bash
curl -fsSL https://github.com/drbh/xmd/releases/latest/download/install.sh | sh
```

[install.sh](install.sh) downloads the release archive for your machine,
checks its sha256 and copies `xmd` into `~/.local/bin`. it uses no sudo and
edits no shell profile; if `~/.local/bin` is not on your PATH it prints the
line to add. run it again to update; `rm ~/.local/bin/xmd` uninstalls. the
same thing by hand:

```bash
mkdir -p ~/.local/bin && \
  curl -fsSL https://github.com/drbh/xmd/releases/latest/download/xmd-$(uname -s)-$(uname -m).tar.gz | \
  tar -xzC ~/.local/bin xmd
```

from a checkout, `cargo install --path hosts/cli` builds the same binary; with
nix, `nix run github:drbh/xmd` or `nix build`.
editors: [vs code](clients/ide/vscode), [zed](clients/ide/zed),
[neovim](clients/ide/neovim), [helix](clients/ide/helix)

rename a `.md` file to `.x.md` and xmd picks it up; editors without xmd
still show it as markdown

## Features

- work in your preferred editor (lsp first)
- cli tool; query docs via command line
- web-based document editor; edit anywhere anytime - local first (offline capable)
- mutable; written in its own language so you can extend and customize it easily

## book

a walk through the whole tool, then a reference. read it at
[xmd.dholtz.com/book](https://xmd.dholtz.com/book/), where every example is
a live note you can edit, or [here on github](book/README.md). start with
[why xmd exists](book/01-blog.md)

## license

MIT, see [LICENSE](LICENSE).
