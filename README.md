[![CI](https://github.com/drbh/jot/actions/workflows/ci.yml/badge.svg)](https://github.com/drbh/jot/actions/workflows/ci.yml)

# wtf

Notes that compute. A `.wtf` file is Markdown where values have names, and
everything that depends on them stays right — in your editor, from the shell,
in a plain text file.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://example.com/placeholder/typing-dark.gif">
  <img alt="Writing a note: values, calculations, dates, tables, tasks, checklists and timers resolve as they are typed" src="https://example.com/placeholder/typing-light.gif" width="720">
</picture>

## The language, in seven lines

```wtf
$1,234:car                          a value with a name
total := car + $67                  a calculation          = $1,301
We have [total] left.               any value in a sentence  $1,301
2026-11-20:departure                dates do arithmetic
- [ ] Pack @due(departure - 14d)    tasks know when they are due  → due 2026-11-06
## Trip :trip                       a named heading counts its tasks  0/1 complete
focus := countdown(25m)             timers are values        ◷ 25:00 remaining
```

The right-hand column is what your editor shows inline. That is the whole
model; everything else is a function you call or a module you add.

## Ask the file a question

```sh
cat trip.wtf | wtf 'total'                        # $1,301
wtf trip.wtf 'tasks | where !done' --json         # typed JSON
wtf --workspace 'diagnostics' --fail-on-match     # a build check
```

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://example.com/placeholder/terminal-dark.gif">
  <img alt="Querying the note from a terminal: single values, filtered tasks, typed JSON, diagnostics for CI and a rendered text export" src="https://example.com/placeholder/terminal-light.gif" width="720">
</picture>

## Install

```sh
cargo install --path lang     # the wtf binary: CLI and language server
```

Editors: [VS Code](client/ide/vscode) · [Neovim](client/ide/neovim) ·
[Zed](client/ide/zed) · [Helix](client/ide/helix) — each runs `wtf lsp`.
Or open a note in the [browser app](client/web) without installing anything.

## Go further

- [Examples](lang/examples) — one short note per feature, in the order you'll meet them.
- [Standard library](lang/stdlib) — the modules behind tasks, timers, plans, feeds and units, written in `.wtf`.
- `wtf --help` and `wtf query --help` — every command, binding, function and stage.
