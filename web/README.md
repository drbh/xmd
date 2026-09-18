# WTF in the browser

A dark-mode, vanilla HTML/JavaScript client. Monaco loads from a pinned ESM CDN;
WTF's existing Rust engine runs inside a dedicated Web Worker as WebAssembly.
The page does not launch or connect to a native WTF language server.

## Build and run

Install Rust and `wasm-pack` if they are not already available, then from the
repository root:

```sh
rustup target add wasm32-unknown-unknown
bash web/build.sh
node web/serve.mjs
```

Open **http://127.0.0.1:4173**. Node only serves static files; any HTTP static-file
server can replace it. Set `WTF_WEB_PORT` to change the development port.
Opening `index.html` through `file://` is not supported because the app uses
module workers and fetches its Wasm module.

The build uses `--no-default-features --features browser` to exclude the CLI,
native filesystem scanning, subprocesses, and Tokio/LSP transport. It creates
`web/pkg/wtf.js` and `web/pkg/wtf_bg.wasm` (plus generated package metadata).
These generated artifacts are ignored by Git; rebuild after changing Rust.
`wasm-pack` selects/caches a matching wasm-bindgen tool version for Cargo.lock.

No npm install or JavaScript bundling is required to run the page. The optional
package.json is for development commands and browser tests only.

## What works

- Inlay values on calculations and prose references, with calculation tooltips.
- Rust-generated semantic highlighting, diagnostics, completion, and signatures.
- Computational tables and `sum(table, row_expression)`, with typed cells,
  scoped column completion/rename, hovers, and undoable Format Document. Import
  `notes/tables.wtf` from the repository for an example; all logic is shared with LSP.
- Definition/reference navigation, rename, and read/write highlights across notes.
- Searchable, collapsible document outline and Monaco's Go to Symbol navigation.
  Both consume the same standard document symbols as the native language server:
  headings, nested tasks, values, timers, and events, with type/value details.
- Contextual task/timer CodeLens controls, including start/pause/resume/reset.
- Extract, inline, freeze-value, and typo-fix code actions.
- Live timer values and clock-dependent diagnostics; ticking never edits source.
- New notes, multi-file import/drop, browser-local autosave, and plain `.wtf` downloads.

`trip.wtf` and `today.wtf` seed a new browser workspace. Existing saved notes are
restored on reload. Duplicate import filenames get a numeric suffix rather than
overwriting an existing note. The storage is per browser profile and origin;
different ports/hosts have different workspaces. Download backups: private-mode
storage, site-data clearing, and storage eviction can remove browser-local notes.
If storage is unreadable, the original value is preserved and autosaving is
disabled. Edits from another tab pause this tab's autosave to prevent silent
overwrites. Downloads do not rewrite files in your Zed workspace.

The worker owns a virtual `/workspace` containing the imported/open notes. Notes
are limited to 1 MB each for language analysis. Source edits use Monaco's undo
history and carry model versions. Multi-note edits validate their targets before
applying; each note has its own undo history. Controls are revalidated against
the latest source before execution, and edits are reported back to Rust.

## Browser differences

- No native filesystem access or folder watching. Import related `.wtf` notes
  together; names resolve among those notes. Local resource links only navigate
  to notes already imported. Local image previews are not implemented.
- GitHub URLs/maps can open in another tab, but GitHub CLI metadata refresh is
  unavailable. No credentials or notes are sent to a language-server backend.
- A local resource's `.exists` produces an explicit unsupported diagnostic,
  rather than pretending the file does not exist on your machine.
- Timers use saved timestamps and catch up after reload or tab suspension.
  Browser throttling can delay visual refreshes. There are no background alarms
  or notifications after the page is closed.
- Monaco assets come from `esm.sh` at version `0.56.0`; first load requires CDN
  access. This is not an offline/PWA build. CDN JavaScript is trusted application
  code; self-host those assets if you need a fully self-contained deployment.
  Markdown hovers are untrusted (no HTML or command execution), and resource
  opening permits only HTTP(S) and imported-note navigation.

## Static deployment

After building, deploy `index.html`, `style.css`, `app.js`, `editor.js`,
`outline.js`, `worker.js`, `monaco-worker.js`, and the generated `pkg/` directory together.
Paths are relative, so a subdirectory deployment works too. Serve `.wasm` as
`application/wasm`. No WebSocket, API route, database, or Rust process is required.
Use HTTPS outside localhost.

The browser bridge uses a small JSON request API, not a second implementation of
the language or a full LSP transport. `src/features/presentation.rs`, `src/features/intelligence.rs`,
`src/features/diagnostics.rs`, and the parser/evaluator/refactor modules are shared with
Zed. `src/hosts/browser.rs` adds virtual-document lifecycle and browser command dispatch.
`editor.js` translates the shared LSP-shaped data into Monaco provider results.
`src/features/symbols.rs` owns document symbols for both hosts. Language/editor features
are LSP-first: implement them in the native server and shared core before exposing
them in the browser. Browser UI must not introduce a separate language parser or
exclusive language behavior. A calendar date-picker widget is not implemented,
because standard LSP does not provide an equivalent portable picker interaction.

## Verification

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
bash web/build.sh
cd web
npm ci
npm test
```

Browser tests use an installed Google Chrome via Playwright (`channel: chrome`)
and exercise the real Wasm worker and rendered Monaco editor. They cover inlays,
editing, reload/save/download, contextual completion/signatures, clickable
controls and undo, timer expiry, cross-note navigation/rename, Unicode filenames,
duplicate imports, and corrupt-storage preservation. Tests need CDN access.
Outline tests cover hierarchy, filtering/collapse, source navigation, live edits,
note switching, and Monaco's actual Go to Symbol picker. Rust tests check the
native LSP response, flat-client fallback, Unicode ranges, and browser/core parity.
