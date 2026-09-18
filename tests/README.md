# Tests

**The test suite is snapshot-only.** Every behaviour is proved end to end, by
running the real `wtf` binary (CLI and language server) or the browser host
against a temporary workspace and recording what comes back. There are no unit
tests that build a `Workspace` in memory and assert on individual values, and no
new ones should be added: if a behaviour cannot be seen from outside the
binary, it is not yet a feature.

One test binary drives everything: `tests/snapshots.rs`.

```sh
cargo test --test snapshots                  # run every case
SNAPSHOT_CASE=charts cargo test --test snapshots   # run one case
UPDATE_SNAPSHOTS=1 cargo test --test snapshots     # rewrite expected.snap
cargo test --features browser --test snapshots     # also run browser cases
```

Failures are reported for *all* cases, not just the first, with a unified line
diff and the path of the temporary workspace, which is kept on failure so it can
be poked at by hand.

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
  silently turn into an LF one.

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

### `{"read": ["path", ...]}`

Records the contents of files in the workspace, or `(missing: …)`. Use it to
prove that a command did not rewrite a note, or that it did write
`.wtf/cache.json`.

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

## Adding a case

1. `mkdir tests/cases/<name>` and drop the notes in.
2. Write `case.json` with the steps that make the behaviour visible.
3. `touch tests/cases/<name>/expected.snap`.
4. `UPDATE_SNAPSHOTS=1 SNAPSHOT_CASE=<name> cargo test --test snapshots`.
5. **Read the snapshot.** It is the test; if it does not show the behaviour you
   meant to pin down, change the steps, not the assertion.
6. Re-run without `UPDATE_SNAPSHOTS` and commit both the case and the snapshot.

Prefer fewer, richer cases: one case with several notes and several steps beats
a dozen near-identical directories.
