# 2. ask a note a question

the default command is a query. pipe a note in or name it

```bash
cat trip.x.md | xmd 'total'                                     # $1,301
xmd trip.x.md 'tasks | filter(fn(t) => !t.done) | map(.{title, due})' --json
xmd --workspace 'diagnostics' --fail-on-match                   # exits 1 on any error
```

a query is one expression, the same language a note writes. the last one
is a ci check. `xmd query --help` lists every binding and function

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/drbh/xmd/releases/download/media/terminal-dark.gif">
  <img alt="querying the note from a terminal: single values, filtered tasks, typed json, diagnostics for ci and a rendered text export" src="https://github.com/drbh/xmd/releases/download/media/terminal-light.gif" width="720">
</picture>

next: [3. notes and libraries](03-files.md)
