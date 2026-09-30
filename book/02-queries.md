# 2. ask a note a question

the default command is a query. pipe a note in or name it

```bash
cat trip.x.md | xmd 'total'                                     #=> $1,301
xmd trip.x.md 'tasks | filter(fn(t) => !t.done) | map(.{title, due})' --json
xmd --workspace 'diagnostics' --fail-on-match
```

`trip.x.md` is the note from [chapter 1](01-language.md). a query is one
expression, the same language a note writes. `#=>` is what a command
prints, and [snapshots/02-queries.txt](snapshots/02-queries.txt) has the
rest. the last one is a ci check: it exits 1 on any error in the
workspace. [the query reference](reference/queries.md) lists every
collection a query can name and the fields its records have

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/drbh/xmd/releases/download/media/terminal-dark.gif">
  <img alt="querying the note from a terminal: single values, filtered tasks, typed json, diagnostics for ci and a rendered text export" src="https://github.com/drbh/xmd/releases/download/media/terminal-light.gif" width="720">
</picture>

next: [3. notes and libraries](03-files.md)
