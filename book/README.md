# The Jot Book

A single-page guide to every feature, with each note from `examples/` inlined
as a live block. The blocks run the same Rust engine as the editor, compiled to
WebAssembly in `web/worker.js`; nothing on the page has its own parser.

```sh
bash web/build.sh        # once, or after changing Rust
node book/build.mjs      # regenerate index.html from examples/ and chapters.mjs
node web/serve.mjs       # then open http://127.0.0.1:4173/book/
```

Code blocks use Ioskeley Mono from `fonts/`, served beside the book.
`chapters.mjs` holds the outline and prose; `build.mjs` writes `index.html`;
`book.js` and `book.css` are the runtime. The generated page is committed so it
can be hosted as static files next to `web/`.
