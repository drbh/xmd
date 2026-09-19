# Tests

**The test suite is snapshot-only.** Every behaviour is proved end to end, by
running the real `wtf` binary (CLI and language server) or the browser host
against a temporary workspace and recording what comes back. There are no unit
tests that build a `Workspace` in memory and assert on individual values, and no
new ones should be added: if a behaviour cannot be seen from outside the
binary, it is not yet a feature.

One test binary drives everything: `tests/snapshots.rs`. Each case is its own
`#[test]`, so cargo runs them in parallel across cores and its usual name filter
picks one out.

```sh
cargo test --test snapshots                  # run every case, in parallel
cargo test --test snapshots charts           # run one case (name filter)
cargo test --test snapshots -- --ignored     # run the browser cases... which need
cargo test --features browser --test snapshots     # ...the feature to do anything
UPDATE_SNAPSHOTS=1 cargo test --test snapshots     # rewrite expected.snap
```

The test functions are generated: `build.rs` lists `tests/cases/*` at build time
and writes one `#[test] fn <name_with_underscores>()` per directory into
`$OUT_DIR/cases.rs`, which `tests/snapshots.rs` includes. A case directory named
`plans-bakery` is therefore the test `plans_bakery`. Adding or removing a case
directory re-runs the build script, so nothing needs registering by hand.

A case with `"requires": "browser"` is emitted as an `#[ignore]`d test unless the
crate is built with `--features browser`, so it is still visible (as `ignored`)
in plain `cargo test` output rather than silently missing. Running it with
`--ignored` but without the feature just prints a skip line: the feature is what
makes those cases do anything.

Each case has its own temporary workspace and its own child processes (the CLI
and `wtf lsp` get their root, clock and `PATH` per `Command`), so cases do not
step on each other when several run at once. Every failing case is reported
independently, with a unified line diff and the path of its temporary workspace,
which is kept on failure so it can be poked at by hand.

## A case

A case is a directory under `tests/cases/<name>/`:

```
tests/cases/charts/
  case.json          the script (required)
  expected.snap      the recorded transcript (required)
  table.wtf          notes, at any depth: notes/a.wtf works too
  .wtf/modules.json  optional module manifest, with .wtf/modules/*.wtf
  .wtf/cache.json    optional resource cache (link metadata)
  .wtf/lookups.json  optional lookup store (rates, quotes, forecasts)
  bin/gh             optional fake programs; made executable and put on PATH
```

Everything except `case.json` and `expected.snap` is copied into a fresh
temporary directory, which becomes the workspace root. Files under `bin/` are
made executable and that directory is prepended to `PATH`, so refresh providers
(`gh`, …) can be faked with shell scripts.

`case.json` is:

```json
{
  "now": "2026-09-16T14:00:00-04:00",
  "requires": "browser",
  "steps": [ ... ]
}
```

- `now` freezes the clock for the whole case (default
  `2026-09-16T14:00:00-04:00`). It is passed to the CLI as `--now` and to the
  language server as `WTF_NOW`, so nothing in a snapshot depends on wall-clock
  time. `TZ` is always `UTC`.
- `requires: "browser"` skips the whole case unless the crate is built with
  `--features browser`. Browser steps belong only in such cases, so that a
  snapshot is byte-identical with and without the feature.

## The transcript

Each step appends a header line `### <index> <kind> <args>` followed by its
output. Two normalizations keep snapshots stable across machines:

- every spelling of the temporary root (plain path and `file://` URI) becomes
  `<root>`;
- carriage returns in recorded note text become `␍`, so a CRLF note cannot
  silently turn into an LF one;
- sub-second wall-clock timestamps (a `refresh` stamping `fetched_at`) become
  `<clock>`;
- the body of every `<style>` element in an HTML export collapses to `…`: the
  web theme is styling, not behaviour, and changes on its own schedule.

## Step kinds

### `{"cli": [...]}`

Runs the built binary with those arguments in the workspace root, and records
the full command line, the exit status, and stdout/stderr (empty streams are
omitted). `--root <workspace>` is appended for subcommands that accept it
(`query`, `render`, `ast`, `graph`, `refresh`) and `--now <now>` for those that
accept a clock (`query`, `render`, `ast`, `graph`) — unless the step already
passes `--root`, `--now` or `--on` itself. `${root}` in an argument expands to
the workspace root.

```json
{ "cli": ["render", "trip.wtf", "--format", "text"] }
{ "cli": ["query", "trip.wtf", "values | select {name, display}", "--json"] }
{ "cli": ["ast", "trip.wtf"] }
{ "cli": ["render", "clock.wtf", "--on", "2026-09-17", "--format", "text"] }
{ "cli": ["refresh"] }
```

A `cli` step may also carry two optional fields. `stdin` feeds text to the
command's standard input (for `query ... -`), and is recorded under `--- stdin`.
`env` sets extra environment variables for that one command, and they are shown
in front of the recorded command line.

```json
{ "cli": ["query", "--workspace", "-", "--json"], "stdin": "tasks | count" }
{ "cli": ["refresh"], "env": {"WTF_TEST_FAIL": "1"} }
```

### `{"read": ["path", ...]}`

Records the contents of files in the workspace, or `(missing: …)`. Use it to
prove that a command did not rewrite a note, or that it did write
`.wtf/cache.json`.

### `{"write": {"path": "text", "other": null}}`

Rewrites the workspace between steps: a string writes that file (creating parent
directories), `null` removes it. Use it to change a module, a manifest or a
cache file halfway through a case, so the next `cli`, `lsp` or `read` step sees
the new disk state.

### `{"lsp": [ ... ]}`

Starts `wtf lsp` once for the step, initializes it with the standard client
capabilities (snippets, hierarchical symbols, refresh support, `showDocument`)
and runs the items in order. Responses and notifications are recorded as pretty
JSON, normalized as above, with `version` fields kept.

| Item | Meaning |
| --- | --- |
| `{"open": "trip.wtf"}` | `didOpen` with the file's text on disk, at version 1 |
| `{"change": {"file": "trip.wtf", "text": "..."}}` | full-text `didChange`, version incremented; the new text is recorded |
| `{"save": "trip.wtf"}` | `didSave` |
| `{"close": "trip.wtf"}` | `didClose` |
| `{"request": "<method>", ...}` | any request; see the params rules below |
| `{"await": "<method>", "file": "trip.wtf"}` | wait for the next notification (or server request) with that method, and that uri if given, and record its params |
| `{"query": {"query": "...", "uri": "trip.wtf"}}` | the custom `wtf/query` method; `uri` is resolved and `now` is added |
| `{"initialize": true}` | records the `initialize` result: what this server told this client it can do |
| `{"notify": "<method>", "params": {...}}` | any client notification (`workspace/didChangeWatchedFiles`, …), with the same params rules |
| `{"write": {"file": "m.wtf", "text": "..."}}` | writes a workspace file behind the server's back, so a watched-file or save notification has something to pick up; the new text is recorded |

An `lsp` step may also carry `"capabilities": {...}`, which replaces the standard
client capabilities for that server, so a case can take the path a poorer editor
takes (`{"lsp": [...], "capabilities": {}}` announces none at all).
| `{"write": {"path": "text"}}` | the `write` step above, in the middle of a session |
| `{"watched": ["path", ...]}` | `workspace/didChangeWatchedFiles` for those paths (changed, or deleted when gone), which is how a module reload is triggered |

A reload is asynchronous, so follow `watched` with an `await` (usually
`textDocument/publishDiagnostics`, which the server sends once the rescan is
done) before requesting anything that should already see the new modules.

Server-initiated requests are answered automatically the moment they are read —
`workspace/applyEdit` with `{"applied": true}` and `window/showDocument` with
`{"success": true}` — and are still available to a later `await`, so
`{"await": "workspace/applyEdit"}` records the edit the server asked for.
Nothing read while waiting for a response is thrown away, so the order of
`request` and `await` items does not matter.

**Request params.** A request builds its params from convenience fields, or
takes a raw `params` object:

- `"file"` → `textDocument.uri`
- `"line"` / `"character"` → `position`
- `"range"` → `range`
- `"extra": {...}` → merged in as-is (`ch`, `options`, `context`, `newName`, …)
- `"params": {...}` → used verbatim instead of all of the above
- `"apply": true` → applies the returned `TextEdit[]` to the open document,
  sends the resulting `didChange`, and records the new text

Placeholders inside `params` and `extra`: `${uri:trip.wtf}` becomes a file URI,
`${root}` the workspace root, `${last}` the previous recorded result and
`${last.0}` its first element — which is how call hierarchy is driven:

```json
{ "request": "textDocument/prepareCallHierarchy", "file": "plan.wtf", "line": 2, "character": 3 },
{ "request": "callHierarchy/outgoingCalls", "params": {"item": "${last.0}"} }
```

Responses to `textDocument/semanticTokens/full` are decoded against the legend
from `initialize` and printed as one readable line per token
(`line:start+length kind [modifiers] "text"`) instead of raw delta integers.

### `{"browser": [ ... ]}`

Only in a case with `"requires": "browser"`. Every `.wtf` note in the case is
loaded with `setDocument` under `/workspace/<relative path>` first, then each
item `{"method": "hover", "params": {...}}` is sent to
`BrowserWorkspace::request` at the frozen clock, and the JSON result recorded.
Any method works, including `setDocument`, `setModules`, `setResourceData`,
`execute` and `query`, so a case can drive a whole session.

Three extras: `"now": "<rfc3339>"` moves that one request's clock (a timer
running out, say); `"body": "<raw text>"` is sent instead of `params`, which is
how malformed input is shown; and `${last:/json/pointer}` inside `params` is
replaced with that part of the previous recorded response, which is how a lens
from an `analyze` is executed:

```json
{ "method": "analyze", "params": {"uri": "file:///workspace/trip.wtf"} },
{ "method": "execute", "params": {"command": "${last:/result/lenses/0/command}",
                                  "versions": {"file:///workspace/trip.wtf": 1}} }
```

`${last:/pointer}` works in an `lsp` request's `params` too, alongside
`${last}` and `${last.0}`.

## Adding a case

1. `mkdir tests/cases/<name>` and drop the notes in.
2. Write `case.json` with the steps that make the behaviour visible.
3. `touch tests/cases/<name>/expected.snap`.
4. `UPDATE_SNAPSHOTS=1 cargo test --test snapshots <name_with_underscores>`.
5. **Read the snapshot.** It is the test; if it does not show the behaviour you
   meant to pin down, change the steps, not the assertion.
6. Re-run without `UPDATE_SNAPSHOTS` and commit both the case and the snapshot.

Prefer fewer, richer cases: one case with several notes and several steps beats
a dozen near-identical directories.
