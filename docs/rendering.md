# Resolved output

`wtf render FILE` exports a saved note as standalone HTML using the language
server's syntax highlighting and inline values. It preserves the source text and
inserts the same inlay labels at the same positions, including calculations,
references, progress bars, timers, solver results, itinerary summaries, cached
link status, and custom plugins.

```sh
wtf render trip.wtf --root notes > trip.html
wtf render trip.wtf --root notes --now 2026-09-18T12:00:00Z > trip.html
wtf render trip.wtf --root notes --on 2026-09-18 > trip.html
wtf render trip.wtf --root notes --format text > trip.txt
```

From this checkout, build with `cargo build --locked` and use
`./target/debug/wtf` in place of `wtf`.

HTML is the default (`--format html`). Open the exported file directly in a
browser, share it, or print it. Its embedded stylesheet uses the shipped editor
palette, semantic token modifiers, and a monospace font. Inlays have distinct
backgrounds and tooltips; diagnostics have wavy underlines and messages. Resource
links remain clickable. No scripts, server, or external assets are required.

For example, this note:

```wtf
[$100]:budget
remaining := budget - $25
Available: [remaining].
```

shows these values, with syntax coloring and separate inlay styling in HTML:

```text
[$100]:budget
remaining := budget - $25 = $75
Available: [remaining] $75.
```

The output is a snapshot: expressions remain visible, just as they do in the IDE.
Colors use the bundled editor palette. Hover explanations are plain tooltip
text; editor commands and timer controls are not interactive in the exported
file. The clock stays at the time
of export. `--format text` emits only the source and inline labels for terminals
or other text tools. Keep the original `.wtf` file for editing and reevaluation.

`FILE` is relative to `--root` (the current directory by default), or an absolute
path to an indexed `.wtf` note in that workspace. The command honors workspace
ignore rules and loads other notes for cross-file references. Save editor changes
first: the CLI reads files on disk. It also loads `.wtf/plugins` and the existing
caches, without refreshing external data or modifying notes or cache files.

Every value uses one clock snapshot. `--now RFC3339` fixes the instant and timezone
offset; `--on YYYY-MM-DD` uses local midnight. These flags are mutually exclusive.
Without either flag, rendering uses the current local time. Match the workspace,
saved files, caches, plugins, and clock to reproduce the editor's inline values.

Both formats preserve Unicode, spacing, and visible line breaks. Text output
also preserves original line endings and the presence or absence of a final
newline. LSP hint padding becomes spaces. Hints at the same position retain their
provider order; label parts concatenate, and label newlines/tabs become spaces.
Rendering never applies a hint's interactive text edits.

Diagnostics go to stderr with source file, line, and column. Notes with errors
still render their successful values, then return exit status 1; warnings alone
return 0. Invalid arguments return 2. Loading or rendering failures return 1
without a document on stdout.

The shared Rust APIs are `rendering::html_in(&request, path)` and
`presentation::render_text_in(&request, path)`. Both use the LSP's `hints_in`
pipeline. `rendering::html(title, document, hints, diagnostics, links)` and
`presentation::render_text(source, hints)` render an existing editor snapshot,
including custom hints. The HTML renderer uses the document's semantic tokens
and escapes all source, labels, tooltips, and link attributes as text.

The HTML browser regression uses the native binary: run `cargo build --locked`,
then `cd web && npx playwright test tests/render.spec.mjs` to check the rendered
text, colors, diagnostics, links, and print styling in Chrome.

HTML serialization now lives in `web/renderer/`, shared by the native CLI and
WASM host. The browser `render`/`analyze` snapshots expose the same fragment with
source, clock, schema, and document versions. The framework-independent JS API
provides `render`, `mount`, and `createWorkspace`; see [web embedding](../web/README.md).
Theme ownership is `web/theme/palette.json`, with scoped CSS shared by the HTML
export, book, and docs and generated adapters for Monaco and Zed.
