# WTF web library and applications

All browser clients, HTML rendering, themes, fonts, and web build tooling live
here. The CLI and browser use the same Rust HTML serializer in `renderer/`.
Language parsing, evaluation, modules, and actions remain in the shared engine.

## Build and run

Install Rust, the `wasm32-unknown-unknown` target, Node, and `wasm-pack`, then:

```sh
npm --prefix web ci
npm --prefix web run build
node web/serve.mjs
```

Open http://127.0.0.1:4173 for the Monaco workspace, `/docs/` for the Svelte
document app, and `/book/` for the live examples. The server serves only
`web/dist/`; it does not launch a language server or mount sibling directories.
Deploy that entire directory to any static server, including beneath a URL
prefix. Serve `.wasm` as `application/wasm`. `WTF_WEB_PORT` changes the local
port; `WTF_WEB_BASE=/notes` exercises a subdirectory deployment.

`npm --prefix web run build:site` rebuilds just the applications and themes using
the existing WASM build. `bash web/build.sh` rebuilds just WASM. One npm lockfile
covers the library and docs app. Every deployed app loads the same
`lib/pkg/wtf_bg.wasm`; `dist/manifest.json` records its SHA-256 hash.

## Use the library

The package is prepared for distribution, but publishing it is a separate step.
Build a local package with `cd web && npm pack`, then install the resulting
`wtf-web-0.1.0.tgz` in a consumer project. The framework-independent ESM API has
TypeScript declarations and no required UI-framework or Monaco dependency.

```js
import { render, mount } from "@wtf/web";
import "@wtf/web/style.css";

// A resolved HTML fragment. Displaying it requires only CSS, with no runtime JS.
const html = await render("total := 2 + 3\n", {
  now: "2026-09-18T12:00:00Z",
});

// Live inlays and core task/timer controls. Source editing is optional.
const view = await mount(document.querySelector("#note"), {
  source: "- [ ] Book the hotel\nfocus := countdown(25m)\n",
  onChange({ uri, source, version }) { save(uri, source, version); },
  onError(error) { console.error(error); },
});
await view.setSource("total := 4 + 5\n");
view.destroy();
```

`render()` also works in Node without a DOM, loading the packaged WASM directly.
It returns `<pre class="wtf"><code>…</code></pre>`; it does not embed a stylesheet.
Use `wtf render FILE` for a complete standalone HTML document with embedded CSS.
Resolving source requires the WASM engine; displaying already resolved HTML does
not. A browser resolving source uses a module worker and needs an HTTP(S) origin.

The default worker URL is relative to the library module, so bundlers can include
it and its WASM dependency. A host with a separate asset pipeline can supply
`workerFactory: () => new Worker(myWorkerURL, { type: "module" })`; deploy the
shipped worker module with its relative imports intact. An advanced `transport`
option supplies the same asynchronous request interface without a Worker.

## Workspaces and views

A view without a supplied workspace owns an isolated workspace and destroys it
on disposal. For cross-note resolution or multiple views, share one explicitly:

```js
import { createWorkspace, mount } from "@wtf/web";

const workspace = createWorkspace({ onError: console.error });
await workspace.setDocument("file:///workspace/budget.wtf", "budget := $100\n");
await workspace.setDocument("file:///workspace/trip.wtf", 'Available [shared.budget].\nshared := import("./budget.wtf")\n');
const view = await mount(element, {
  workspace,
  uri: "file:///workspace/trip.wtf",
  layout: "document",
});
const unsubscribe = workspace.onChange(({ uri, source }) => save(uri, source));

view.destroy();       // Detach this view; documents and other views survive.
unsubscribe();
workspace.destroy();  // Stop the worker and all subscriptions.
```

Names are local to each document. Imports use paths relative to that document;
supply imported sources with `setDocument` before rendering. A missing source
produces a diagnostic; the browser never fetches it automatically.

Documents use file URIs beneath `/workspace/`, end in `.wtf`, and are limited to
1 MB each. The controller allocates monotonically increasing document versions,
serializes transport calls, rejects stale edits, invalidates dependent views,
and runs one live-refresh scheduler per workspace. `onChange` fires for source
transactions; `onRender` fires for presentation updates, including clock ticks.
Persist by the callback's URI, never by the currently selected document.

Use `setModules({ "custom.wtf": source })` for the shared language's module
interface and `setResourceData(url, data)` for host-supplied resource metadata.
Both invalidate presentation without emitting source changes. `query`,
`analyze`, and `request` expose the existing browser engine protocol for advanced
integrations. Snapshots carry their source, URI, document/workspace versions,
clock, schema version, tokens, hints, diagnostics, links, controls, and HTML.
Treat snapshots as read-only.

A fixed RFC3339 `now` string freezes evaluation and disables ticking. A function
can supply a custom live clock. Browser defaults use local time; Node rendering
defaults to UTC. Use an explicit clock for reproducible output. `render()` uses
strict diagnostics by default; editable views use the engine's editing mode,
which suppresses some incomplete-expression diagnostics.

## Editing and appearance

`mount()` displays resolved content with clickable links and versioned core
controls. Set `interactive: false` to omit controls, or `controls: false` to hide
the control bar while retaining inline checkbox actions. `onOpen(url)` delegates
navigation to the host; the default opens HTTP(S) links in another tab.

For lightweight source editing:

```js
import { mountEditor } from "@wtf/web/contenteditable";
const editor = await mountEditor(element, { workspace, uri, onError });
await editor.insertAtCaret("\n- [ ] Next task\n");
await editor.undo();
await editor.redo();
editor.destroy();
```

This optional adapter preserves selections, pauses repainting during composition,
inserts plain text, and owns source undo/redo. Monaco remains the full IDE adapter
at `@wtf/web/monaco`, with an optional `monaco-editor` peer dependency. Its
`createEditor(element, client)` returns an editor, a per-instance `language` ID,
and `destroy()`; use that ID for its models. The workspace app shows the complete
client adapter, including mapping workspace versions to Monaco's undo versions.
The demo loads pinned Monaco assets from a CDN; the base library, docs, and book
do not depend on that CDN.

`style.css` scopes semantic styling to `.wtf`. Customize `--wtf-background`,
`--wtf-foreground`, `--wtf-font`, `--wtf-font-size`, `--wtf-inlay-background`, and
`--wtf-inlay-foreground` on the host. `layout: "source"` preserves unwrapped
source lines; `"document"` wraps lines and sizes headings using parser metadata.
Optionally import `@wtf/web/fonts.css` for the bundled Ioskeley Mono fonts.

`theme/palette.json` is the source for HTML, Monaco, and generated Zed token
styles. Regenerate adapters with `node web/scripts/theme.mjs`. Application CSS
owns page layout; it does not maintain separate semantic token palettes.

## Boundaries and verification

The browser workspace is virtual: it does not scan files, watch folders, invoke
native commands, or fetch external metadata automatically. Import related notes
and supply resource data explicitly. Features and actions are the same core
implementations used by LSP, subject to the browser host's capabilities.
App persistence, file pickers, navigation, and page layout remain outside the
library. Both persistence demos preserve unreadable storage and pause saving
when another tab changes it.

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
npm --prefix web run build
npm --prefix web test
```

Playwright uses installed Google Chrome. Monaco tests require CDN access.
Tests cover native/browser rendering parity, core actions, editor operations,
UTF-16 coordinates, worker failure, disposal, live document switching,
subdirectory deployment, persistence, and static HTML without external requests.
