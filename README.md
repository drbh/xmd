# xmd

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/drbh/xmd/releases/download/media/typing-dark.gif">
  <img alt="a finished trip note: one number changes and the total follows, the departure date changes and the countdown and due date follow, then tables, checklists and timers are typed in" src="https://github.com/drbh/xmd/releases/download/media/typing-light.gif" width="720">
</picture>

extended markdown is an opinionated tool for better notes.

[why xmd](https://xmd.dholtz.com).

## install

the prebuilt cli and language server, for mac and linux.

```bash
curl -fsSL https://xmd.dholtz.com/install.sh | sh
```

| editor                        | setup                                              |
| ----------------------------- | -------------------------------------------------- |
| [vs code](clients/ide/vscode) | replace `sh` above with `sh -s -- --editor vscode` |
| [zed](clients/ide/zed)        | replace `sh` above with `sh -s -- --editor zed`    |
| neovim                        | [setup guide](clients/ide/neovim)                  |
| helix                         | [setup guide](clients/ide/helix)                   |
| web                           | [open editor](https://xmd.dholtz.com/docs/)        |

the [installer](installer) is cached for repeat runs

## develop

with git, make, and Rust installed through rustup:

```bash
git clone https://github.com/drbh/xmd.git
cd xmd
make
export PATH="$PWD/target/release:$PATH"
```

this builds `xmd`, including `xmd lsp`. launch your editor from this shell to use it

build your client with `make zed`, `make vscode`, `make neovim`, `make helix`, or [`make web`](clients/web). VS Code and web need Node.js/npm; web also needs wasm-pack

`make all` builds everything; `make help` lists the options
