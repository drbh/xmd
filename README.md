# xmd

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/drbh/xmd/releases/download/media/typing-dark.gif">
  <img alt="a finished trip note: one number changes and the total follows, the departure date changes and the countdown and due date follow, then tables, checklists and timers are typed in" src="https://github.com/drbh/xmd/releases/download/media/typing-light.gif" width="720">
</picture>

markdown notes that know what they say: money, dates, durations, tasks and
tables become live values in the editor you already use. a note stays a
plain `.x.md` file that reads fine anywhere without xmd

**start with [why xmd exists](https://xmd.dholtz.com/book/blog)**
([on github](book/01-blog.md)). it walks through the whole tool one live
note at a time and links to everything else; this readme only covers
installing

## install

the cli and language server, for mac and linux:

```bash
curl -fsSL https://github.com/drbh/xmd/releases/latest/download/install.sh | sh
```

[install.sh](install.sh) is short; read it first if you like. it checks the
download's sha256, copies `xmd` into `~/.local/bin` and changes nothing
else. run it again to update; `rm ~/.local/bin/xmd` uninstalls. or build
with cargo, `cargo install --git https://github.com/drbh/xmd xmd`, or with nix,
`nix run github:drbh/xmd`

then your editor: [vs code](clients/ide/vscode), [zed](clients/ide/zed),
[neovim](clients/ide/neovim), [helix](clients/ide/helix). or skip installing
and use the [web editor](https://xmd.dholtz.com/docs/)

## reference

[the book](book/README.md) has the reference: functions, libraries,
collections and writing modules

## license

MIT, see [LICENSE](LICENSE).
