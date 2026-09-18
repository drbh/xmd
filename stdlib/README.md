# Standard library

Features are first-class WTF modules. The files here are bundled into the native
CLI, language server, and browser engine and use the same compiler and evaluator
as workspace modules.

- `kind: "library"` exports functions for `import("id")`.
- `kind: "feature"` reads document/query records and supplies inlays (`collect`),
  `hovers`, `diagnostics`, `format`, `actions`, and action execution (`reduce`).
- `kind: "link"` resolves known hosts and supplies their labels, properties,
  hovers, and optional refresh requests.

Every module declares `module := {api: 1, id: "…", kind: "…"}`. Dependencies are
explicit: add `imports: ["format"]`, then use `fmt := import("format")`.
Document inputs can select collections and individual fields, for example
`inputs: {sections: ["anchor", "title"]}`. These are the same records as queries.

## Loading and replacement

The loader starts with the bundled library, overlays saved `stdlib/*.wtf` files
from workspace roots, then overlays `.wtf/modules/*.wtf`. Matching IDs replace
whole modules; all consumers are relinked against that same snapshot, including
transitive imports. `enabled: false` disables an ID without falling back to the
bundled version. Module files are kept out of the note index.

The language server reloads saved changes atomically and retains the last good
snapshot on compilation errors. Unsaved module buffers receive highlighting
without changing running features. Editing this repository's `stdlib/` therefore
works immediately when it is opened as the workspace root; rebuilding embeds the
changes for other workspaces and browser deployments.

Browser applications call `workspace.setModules({"name.wtf": source})`. This
replaces the workspace module sources atomically over the embedded library. An
empty object restores the bundled modules. Invalid updates retain the last good
snapshot. Actions carry `module`, `revision`, and an event; execution rejects
controls made for an older module or dependency revision.

## Example

Save this as `.wtf/modules/headings.wtf`:

```wtf
module := {
  api: 1,
  id: "headings",
  kind: "feature",
  inputs: {sections: ["anchor", "title"]}
}

// Show a compact character count beside each heading.
collect := fn(ctx) => (
  map(ctx.document.sections, fn(s) => {
    at: s.anchor,
    label: text(length(s.title)) + " characters"
  })
)
```

Run `wtf render note.wtf --root . > note.html` to inspect exactly the same inlays,
syntax tokens, links, and diagnostics as the editor. Module functions are pure:
hosts validate their returned data and execute explicit action/refresh requests.

## Runtime boundary

All default inlays are feature modules: `definitions`, `references`, `tasks`,
`links`, `checklists`, `calculations`, `table_cells`, `plans`, `timers`, and
`itinerary`. Disabling them removes their hints; there is no second native inlay
implementation to take over.

The `agenda` library selects and orders tasks, events, and itinerary stops over
an explicit date window. The editor and CLI call the same `between` function; see
the [query guide](../src/features/query.md) for examples.

The `plan`, `timer`, and `itinerary_core` libraries provide solver policy, timer
construction/properties/transitions, and itinerary resolution. Typed adapters
call the active workspace library. A timer retains the module snapshot that
created it. Its `time_dependent(timer)` export controls clock refreshes; absent
that export, clock use in the module or its imports determines refreshes.

Rust retains the language parser and evaluator, typed value adapters, document
and query records, the numeric `solve_linear` primitive, generic editor operations,
and host I/O. Modules return data; the host validates positions and edits and
executes authorized actions. A library error is reported instead of selecting a
bundled replacement.

Feature inputs also expose these reusable query fields:

- `values` and `references`: `display`, `hover`, `type`, `errors`, and UTF-16
  `anchor`; `presentation` is a resource view or null.
- `links`: `presentation` contains `label`, `hover`, and `known`, resolved from
  the active host modules and cached data.
- `tasks`: `schedule` keeps explicit date attributes and their errors;
  `blocked_by`, `blocked_error`, `children`, and `timer` expose evaluated state.
  The module decides how to combine and display them.

The HTML regression fixture records output before the migration. Additional
native and browser tests replace library exports to prove that evaluation,
actions, queries, and rendering use the same implementation.
