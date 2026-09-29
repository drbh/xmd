# 4. modules

a module is a `.xmd` library that adds to xmd: inlays, hovers,
diagnostics, functions, commands. no build step, the language server
loads it

```xmd
module := {api: 1, id: "wordcount", kind: "feature", inputs: {notes: ["anchor", "text"]}}
collect := fn(ctx) => map(ctx.document.notes, fn(n) => {at: n.anchor, label: "hi"})
```

list it in a `modules.json`, paths relative to that file

- every note you open: `~/.config/xmd/modules.json`
- one workspace: `.xmd/modules.json` at its root

`kind` is `feature` (annotates notes), `link` (urls on some `hosts`),
`library` (functions a note gets with `import(id)`), `command` (`xmd run`)
or `provider` (rates, quotes, forecasts). a module with a stdlib id
replaces it

try one: `cp -r lang/examples/personal/xmd ~/.config/` puts a word count
after every note's title, then restart the language server. more in
`lang/examples/modules` and `lang/stdlib`

next: [5. put a note on your own page](05-embed.md)
