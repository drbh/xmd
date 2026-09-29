# xmd docs

a plain text note format with a language server. the numbers, dates and
tasks in a `.x.md` file have names, so when one changes the rest follow,
in your editor and from the shell

for anyone who keeps a trip, a budget or a project in a text file and has
found the number in it was wrong. say a note was written days ago;
change `$67` and the total follows, change the date and the countdown and
the due date follow. everything else is one of seven things

```xmd
$1,234:car                          a value with a name
total := car + $67                  a calculation          = $1,301
We have [total] left.               any value in a sentence  $1,301
2026-11-20:departure                dates do arithmetic
## Trip :trip                       a named heading counts its tasks  0/1 complete
- [ ] Pack @due(departure - 14d)    tasks know when they are due  due 2026-11-06
focus := countdown(25m)             timers are values        25:00 remaining
```

the right column is what the editor shows inline. thats the whole language;
the rest is functions you call and modules you add

## ask the file a question

the default command is a query. pipe a note in or name it

```bash
cat trip.x.md | xmd 'total'
xmd trip.x.md 'tasks | where !done | select {title, due}' --json
xmd --workspace 'diagnostics' --fail-on-match
```

```
$1,301
[
  {
    "due": {
      "type": "date",
      "value": "2026-11-06"
    },
    "title": "Pack"
  }
]
```

the last one exits 1 when any note has an error, so it works as a ci check

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/drbh/jot/releases/download/media/terminal-dark.gif">
  <img alt="querying the note from a terminal: single values, filtered tasks, typed json, diagnostics for ci and a rendered text export" src="https://github.com/drbh/jot/releases/download/media/terminal-light.gif" width="720">
</picture>

## install

mac and linux:

```bash
curl -fsSL https://github.com/drbh/jot/releases/latest/download/install.sh | sh
```

that is the cli and the language server in one binary, `xmd`, in `~/.local/bin`.
or grab a binary from the releases page:
https://github.com/drbh/jot/releases

editors:

- vs code: install the extension from the `.vsix` on the releases page
  (Extensions view > ... > Install from VSIX); it downloads the server itself
- zed: install the extension (unpack `xmd-zed.tar.gz` from the releases page,
  Extensions > Install Dev Extension > that folder); it downloads the server itself
- neovim and helix: install the binary with the one-liner, then the config file
  in `client/ide/neovim` or `client/ide/helix`

from a checkout, `cargo install --path lsp` builds the same binary.
`client/web` is the browser app if you dont want to install anything

## put a note on your own page

three lines, no build step, nothing on your server. the engine loads from the
hosted app into a worker in the page

```html
<link rel="stylesheet" href="https://xmd.dholtz.com/lib/theme/style.css">
<div id="note"></div>
<script type="module">
  import { mountEditor } from "https://xmd.dholtz.com/lib/adapters/contenteditable.js";
  mountEditor(document.querySelector("#note"), { source: "rent := $900\nfood := $250\nTotal [rent + food]\n" });
</script>
```

`mount` gives a read-only view, `render` static html, `onChange` tells you what
people typed. https://xmd.dholtz.com/embed/ is that page, live

## when not to use it

its a file. there is no sync, no accounts and no phone app unless you use
the hosted app, and the browser cannot fetch feeds, rates or github status
(that needs the cli, `xmd refresh`). money math is decimal but not
accounting-grade, and the solver does linear plans only. if your notes are
mostly prose with no numbers in them, markdown is already enough

## go further

in an editor, signature help and completion describe every function and
attribute as you type. `lang/examples` has one short
note per feature, numbered in the order you tend to meet them. `lang/stdlib`
is the standard library, tasks, timers, plans, feeds and units are all
written in `.x.md` and you can replace any of them. `xmd --help` and
`xmd query --help` list every command, binding, function and stage
