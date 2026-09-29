# modules

a module is a `.x.md` file that adds to xmd: inlays, hovers, diagnostics,
functions for notes, commands. no build step, the language server loads it

list modules in a `modules.json`, paths relative to the file:

- personal, every note you open: `~/.config/xmd/modules.json`
- one workspace: `.xmd/modules.json` at its root

```
module := {api: 1, id: "wordcount", kind: "feature", inputs: {notes: ["anchor", "text"]}}
collect := fn(ctx) => map(ctx.document.notes, fn(n) => {at: n.anchor, label: "hi"})
```

`kind` is `feature` (annotates notes), `link` (urls on some `hosts`),
`library` (functions a note gets with `import(id)`), `command` (`xmd run`) or
`provider` (rates, quotes, forecasts). a module with a stdlib id replaces it

try one: `cp -r lang/examples/personal/xmd ~/.config/` puts a word count after
every note's title. more in `lang/examples/modules` and `lang/stdlib`.
restart the language server after changing personal modules
