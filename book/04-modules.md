# 4. modules

a module is a `.xmd` library that adds to xmd: inlays, hovers,
diagnostics, functions, commands. no build step, the language server
loads it

```xmd wordcount.xmd
module := {api: 1, id: "wordcount", kind: "feature", inputs: {notes: ["anchor", "text"]}}
collect := fn(ctx) => map(ctx.document.notes, fn(n) => {at: n.anchor, label: "hi"})
```

list it in a `modules.json`, paths relative to that file

- every note you open: `~/.config/xmd/modules.json`
- one workspace: `.xmd/modules.json` at its root

`kind` is `feature` (annotates notes), `link` (urls on some `hosts`),
`library` (functions a note gets with `import(id)`), `command` (`xmd run`)
or `provider` (fetches what `cached(kind, key)` asks for: rates, quotes,
forecasts, or a kind of your own). a module with a stdlib id replaces it

one library needs no `import`: the exports of `prelude`
([lang/stdlib/prelude.xmd](../lang/stdlib/prelude.xmd)) are names in every
note and module, which is where `sparkline`, `total`, `effort`, `rate`,
`to`, `quote` and `forecast` come from. a note's own names come first, so a note that defines `total` reads
its own

try one: `cp -r lang/examples/personal/xmd ~/.config/` puts a word count
after every note's title, then restart the language server. more in
`lang/examples/modules` and `lang/stdlib`

next: [5. put a note on your own page](05-embed.md)
