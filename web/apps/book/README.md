# The WTF Book

The book inlines the language examples into editable views of one shared
`@wtf/web` workspace. It uses the public contenteditable adapter and shared theme.

From the repository root:

```sh
npm --prefix web ci
npm --prefix web run build
node web/serve.mjs
# Open http://127.0.0.1:4173/book/
```

`chapters.mjs` owns prose and example ordering. `build.mjs` reads the root
`examples/` fixtures and writes generated HTML to `web/dist/book/`. The shared
site build adds runtime modules, styles, fonts, and the single WASM artifact.
Generated HTML is not committed. See [the web library](../../README.md) for
embedding the same views elsewhere.
