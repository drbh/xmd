[![CI](https://github.com/drbh/jot/actions/workflows/ci.yml/badge.svg)](https://github.com/drbh/jot/actions/workflows/ci.yml)

# wtf

notes that compute. a `.wtf` file is markdown where values have names and
anything that depends on them stays current, in your editor and from the
shell. its still a plain text file

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://example.com/placeholder/typing-dark.gif">
  <img alt="writing a note: values, calculations, dates, tables, tasks, checklists and timers resolve as they are typed" src="https://example.com/placeholder/typing-light.gif" width="720">
</picture>

## the language in seven lines

```wtf
$1,234:car                          a value with a name
total := car + $67                  a calculation          = $1,301
We have [total] left.               any value in a sentence  $1,301
2026-11-20:departure                dates do arithmetic
- [ ] Pack @due(departure - 14d)    tasks know when they are due  → due 2026-11-06
## Trip :trip                       a named heading counts its tasks  0/1 complete
focus := countdown(25m)             timers are values        ◷ 25:00 remaining
```

the right column is what the editor shows inline. thats the whole model,
the rest is functions you call and modules you add

## ask the file a question

the default command is a query. pipe a note in or name it

```bash
cat trip.wtf | wtf 'total'
wtf trip.wtf 'tasks | where !done | select {title, due}' --json
wtf --workspace 'diagnostics' --fail-on-match
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
  <source media="(prefers-color-scheme: dark)" srcset="https://example.com/placeholder/terminal-dark.gif">
  <img alt="querying the note from a terminal: single values, filtered tasks, typed json, diagnostics for ci and a rendered text export" src="https://example.com/placeholder/terminal-light.gif" width="720">
</picture>

## install

```bash
cargo install --path lang
```

that is the cli and the language server in one binary. editor setups are in
`client/ide/vscode`, `client/ide/neovim`, `client/ide/zed` and
`client/ide/helix`, each runs `wtf lsp`. `client/web` is the browser app if
you dont want to install anything

## go further

`lang/examples` has one short note per feature, numbered in the order you
tend to meet them. `lang/stdlib` is the standard library, tasks, timers,
plans, feeds and units are all written in `.wtf` and you can replace any of
them. `wtf --help` and `wtf query --help` list every command, binding,
function and stage
