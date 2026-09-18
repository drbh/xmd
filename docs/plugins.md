# Functional plugins

Plugins are `.wtf` modules using the same [functional expressions](functions.md)
as notes and queries. Put modules directly in `<workspace>/.wtf/plugins/`.
No Rust build, separate manifest format, or language runtime dependency is needed.
Each definition occupies one line, like ordinary note calculations.

For a custom URL host, save this as `.wtf/plugins/docs.wtf`:

```wtf
plugin := {api: 1, id: "docs", kind: "link", hosts: ["docs.example"]}
inlay := fn(ctx) => "docs · " + ctx.url.path
hover := fn(ctx) => "Documentation at " + ctx.url.raw
```

A link plugin implements the existing `LinkFeature` contract through the module
adapter. Its semantics apply to raw links, Markdown links, named resources,
aliases, references, property completion and expression evaluation. Exact,
lowercase host names match HTTP and HTTPS URLs; optional `path_prefix` narrows
matching. Modules are ordered by file path, precede built-ins, and the first
matching provider wins. Plugin IDs must be unique across the workspace roots.

`ctx.url` contains `raw`, `host`, `path` and `scheme`. `ctx.cached` is a record or
`null`; `ctx.fetched_at` is a timestamp or `null`. Inlay and hover functions return
text. Use `now()` for the injected clock; modules that mention `now` or `today`
participate in live inlay refreshes. Link clocks use UTC, as the link trait does.
General inlays use the host request's timezone offset.

## Refresh and properties

The language describes a refresh as data. Only an explicit native refresh action
executes the program, with separate arguments and the existing timeout. Rendering
never executes the request. `./` and `../` programs resolve relative to the module;
other program names use the native host's executable search path. Native refresh
programs inherit the host environment, so installed CLI tools can use their normal
authentication. A refresh program has the same access as that native host.

```wtf
plugin := {api: 1, id: "tickets", kind: "link", hosts: ["issues.example"], properties: ["title", "points"], cache_version: 1}
inlay := fn(ctx) => if(ctx.cached == null, "ticket · refresh", ctx.cached.title)
property := fn(ctx, name) => get(ctx.cached, name)
refresh := fn(url) => {program: "ticket-cli", args: ["show", url.raw, "--json"]}
decode := fn(url, data) => {title: data.summary, points: data.points}
```

The program writes JSON to stdout. `decode` returns a record containing JSON
values, stored with the plugin ID, cache version and fetch time. Property functions
can derive ordinary typed WTF values from that data. Change `cache_version` when
changing the cached record's meaning; mismatched cached data is treated as absent.
The bundled GitHub module reads legacy cache entries; ordinary plugins use their
own ID/version namespace.
Both `refresh` and `decode` must be supplied together. Refresh failures and decoder
errors preserve the previous cache. Refresh descriptions depend only on their URL;
clock functions in refresh descriptions use a fixed epoch.

## Other inlays

A module with `kind: "inlay"` implements the existing `InlayFeature` contract:

```wtf
plugin := {api: 1, id: "headings", kind: "inlay"}
collect := fn(ctx) => map(ctx.document.sections, fn(h) => {line: h.line, label: "section · " + h.title, tooltip: "From headings.wtf"})
```

`ctx.document` contains `path`, `lines`, `sections`, `tasks`, `links`, and
`definitions`. Sections contain `line` and `title`; tasks also contain `checked`;
links contain `line` and `url`. Definitions contain `line`, `name`, `value`, and
`error` (null on success). Lines are zero-based. `collect` returns a list of records
with `line`, text `label`, and optional text `tooltip`. Hints appear at the end of
that line. The shared sink handles UTF-16 positions, range filtering, and sorting.
Invalid output produces an error hint instead of partial output or source edits.

## Reloading and hosts

The CLI loads modules for each invocation. The native editor reloads saved modules
on save notifications and watched-file events, including creation and deletion.
Clients supporting watched-file registration receive an explicit plugin glob.
Plugin files stay outside the ordinary note symbol catalog. Their unsaved buffers
receive semantic highlighting, including bundled standard-library files, but do
not change the active registry. Invalid reloads retain the complete previous
registry and report the error in the language server log.

Each workspace snapshot holds an immutable registry behind an `Arc`. Replacement
is atomic, unchanged sources retain the same registry, and requests already using
an older snapshot finish against it. A native refresh that outlives a registry
change is rejected before replacing the active workspace cache.

Browser hosts use the same Rust interpreter and adapters. Send a complete module
replacement using the worker API:

```json
{"method":"setPlugins","params":{"sources":{"docs.wtf":"plugin := {api: 1, id: \"docs\", kind: \"link\", hosts: [\"docs.example\"]}\ninlay := fn(ctx) => ctx.url.path"}}}
```

At the Rust boundary this is `request("setPlugins", JSON_payload, timestamp)`;
use the `params` object as the payload. Empty `sources` unloads all plugins.
Invalid replacements return an error and retain the prior registry. Browser hosts
can supply externally fetched JSON through `setResourceData` with `{url, data}`;
the registered provider decodes it. The browser never executes refresh programs.

The interpreter restricts plugin evaluation to supplied data and the request
clock. There is no filesystem or process access from expressions. Modules are
limited to 64 KiB, with at most 64 modules, bounded text/collections, evaluation
steps, expression depth and function call depth. Libraries support explicit imports;
there are no mutable variables, asynchronous expressions, or third-party runtime.

A working workspace is in [`examples/plugins`](../examples/plugins/demo.wtf).

## Shared semantic inputs and replacement

Set `inputs: ["sections", "tasks"]` to select the catalog collections needed by a
module. They are exposed under `ctx.document` using exactly the same records as
queries. Available collections include `values`, `plans`, `tables`, `rows`,
`tasks`, `events`, `stops`, `days`, `timers`, `decisions`, `entries`, `resources`, `notes`, `diagnostics`, `links`,
`sections`, `calculations`, `references`, and `cells`. Values retain their units;
evaluated expression records also include `display`, `type`, and `errors`.
Sections expose `end_line`; tasks expose evaluated `done`, `leaf`, and `estimate`.

Every record has a zero-based `line` and UTF-16 `anchor`; `source.range` identifies
its source text. Return `{at: record.anchor, label: "...", tooltip: "..."}` for an
exactly positioned hint. Legacy `{line: n, label: "..."}` still means line end.
Malformed positions, including split surrogate pairs, reject the whole batch.
`ctx.range` is the requested hint range. `ctx.document` always supplies `path`,
`uri`, `text`, and `lines`; `ctx.today` is the request calendar date. Omitting `inputs` selects the original collections;
`definitions` remains an alias for `values` with a nullable `error` field.

A workspace module replaces a provider with the same ID. Inlay provider IDs are
`definitions`, `plans`, `timers`, `table_cells`, `itinerary`, `checklists`,
`tasks`, `calculations`, `references`, and `links`; the GitHub link provider is
`github`. Replacement is by provider, independent of which records or URLs the
replacement chooses to handle. To disable a provider, install only:

```wtf
plugin := {api: 1, id: "checklists", kind: "inlay", enabled: false}
```

Removing a replacement restores the default provider on the next successful
reload. Distinct IDs extend the registry. The same rules apply in the browser.

Link modules may additionally define `matches(url)` to narrow host matching,
`property_names(url)` to return a subset of declared `properties`, and
`time_dependent(ctx)` to control clock refreshes precisely. `refresh(url)` may
include text `title` and an `env` record of text environment variables. The
optional `cache_namespace: null` opts into legacy, unnamespaced metadata; omit it
for the normal ID/version isolation. Legacy metadata is exposed as a cached
record when it has no generic `data` payload. `ctx.native` indicates whether the
runtime can execute native refresh programs.

## Interactive features

Inlay modules can define `actions(ctx)` alongside `collect(ctx)`, or on its own.
The context adds zero-based `row` and `capabilities: {refresh, views}`. Actions
appear in the existing code actions, lenses, and browser controls. Return a list
of `{title, action}` records; actions use the shared tagged action vocabulary:
`toggle_task`, `timer`, `open_resource`, `refresh_resource`, `refresh`,
`show_today`, `edit`, and `invoke`. Fields mirror `commands::Action`; for example:

```wtf
plugin := {api: 1, id: "greeting", kind: "inlay", inputs: []}
actions := fn(ctx) => if(ctx.row == 0, [{title: "Insert greeting", action: {kind: "edit", document: ctx.document.uri, expected: ctx.document.text, edits: [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 0}}, newText: "Hello "}]}}], [])
```

`edit` requires the complete expected source snapshot and LSP `TextEdit` records.
The host validates document membership, exact source, UTF-16 boundaries, range
ordering, and overlap before applying edits. Existing action types retain their
own semantic checks. Unsupported host actions are omitted; an invalid proposal
rejects that module's entire action batch. Execution revalidates against current
source, so stale controls cannot overwrite newer edits. Merely evaluating a
module never applies an action.


## Bundled modules and the core

The shipped [GitHub provider](../stdlib/.wtf/plugins/github.wtf),
[calculations](../stdlib/.wtf/plugins/calculations.wtf),
[checklist progress](../stdlib/.wtf/plugins/checklists.wtf), and
[computed table cells](../stdlib/.wtf/plugins/table_cells.wtf) are ordinary WTF
modules. They are embedded for native and browser builds, compiled by the same
module loader, and run through the same adapters and limits as user modules.
Copy one into `.wtf/plugins/` and edit it to replace that default; no Rust
registration or build is needed. Regression fixtures compare their behavior with
the previous Rust implementations, which have been removed from production.

Inputs can also select fields, using catalog field names:

```wtf
plugin := {api: 1, id: "completion-count", kind: "inlay", inputs: {tasks: ["done"], sections: ["anchor"]}}
collect := fn(ctx) => map(ctx.document.sections, fn(s) => {at: s.anchor, label: text(length(filter(ctx.document.tasks, fn(t) => t.done))) + " completed in this note"})
```

Field selection avoids copying unused catalog data into a module and leaves more
room within the bounded value size. Unknown fields produce a module error.
The kernel owns parsing, typed evaluation, semantic records, source coordinates,
output validation, and effect delivery. Modules own the migrated presentation
and link policies, timer transitions, plan construction and interpretation, and
itinerary calendar rules. Rust retains typed symbolic lowering, numerical solving,
calendar primitives, task/reference infrastructure, and source-safe host effects.
There is no plugin callback for adding parser syntax.

## Libraries, reducers, and additional hooks

A `kind: "library"` module exports its definitions (except `plugin` and names
starting with `_`). Declare dependencies with `imports: ["format"]`, then write
`fmt := import("format")`. Imports form an acyclic graph, linked atomically with
the registry. Functions retain their module's lexical environment, including
when they return closures. Calls across modules share the same evaluation step
and call-depth limits. Module expressions are parsed once per compiled snapshot.
Notes and queries can also call `import("format").human(125m)`.

For an action that needs fresh state, return this action shape from `actions`:

```wtf
{kind: "invoke", document: ctx.document.uri, expected: ctx.document.text, plugin: ctx.plugin.id, revision: ctx.plugin.revision, event: {operation: "start"}}
```

`reduce(ctx, event)` runs when the action is executed and returns one concrete
shared action, such as `edit`. Its clock and semantic inputs belong to that
execution request. The host checks the expected source and the module/dependency
revision before calling it, then validates the returned effect. Reducers cannot
return another invocation. Listing controls does not run reducers.

Inlay modules may implement any of these callbacks without `collect`:

| Callback | Output |
| --- | --- |
| `hovers(ctx)` | `[{range, contents: "Markdown"}]` |
| `diagnostics(ctx)` | LSP diagnostic records, with `range`, `message`, and optional numeric `severity` / `code` |
| `format(ctx)` | LSP text edits |

All use the same selected catalog inputs as `collect`. Positions and edit ranges
are validated; an invalid batch is rejected. Formatting rejects overlaps across
providers, including native table formatting. User hovers take precedence over
native fallbacks. The `diagnostics` input collection contains native diagnostics,
so a diagnostic provider cannot recursively request its own output.

Pure computational primitives include `solve_linear(model)` (raw linear solver
status and values), `date_parts`, `make_date`, `at_time`, `parse_time`, and
`parse_duration`, `duration_parts`, `parse_date`, and `parse_datetime`. `entries`,
`object`, `number`, `source`, `pad_start`, and `pad_end` support
ordinary structured transformations and formatting. These primitives are equally
available in notes, queries, and modules. `solve_linear` accepts named variables
with explicit continuous/integer/binary kinds, optional lower/upper bounds, a
linear objective `{constant, terms}`, and constraints `{lhs, op, rhs}`. An optional
`order` lists every variable exactly once to preserve ordering when a model has
multiple optimal solutions. Its
statuses are `optimal`, `infeasible`, and `unbounded`; optimal results include a
`values` record. Models are limited to 512 variables and 2048 constraints, within
the language's existing value-size limit.

## Solver, timer, and itinerary libraries

The bundled features are built from ordinary libraries and inlay providers:

| Library | Behavior | Provider |
| --- | --- | --- |
| [`plan`](../stdlib/.wtf/plugins/plan.wtf) | Variable defaults, model assembly, solver statuses, typed results, goal seeking, constraint/choice labels, hover and write-choice edits | [`plans`](../stdlib/.wtf/plugins/plans.wtf) |
| [`timer`](../stdlib/.wtf/plugins/timer.wtf) | Constructor validation, elapsed time, state, properties, controls, transitions and presentation | [`timers`](../stdlib/.wtf/plugins/timers.wtf) |
| [`itinerary_core`](../stdlib/.wtf/plugins/itinerary_core.wtf) | Year inference, cancellation deadlines, gaps, layovers, relative dates, hovers, diagnostics and formatting | [`itinerary`](../stdlib/.wtf/plugins/itinerary.wtf) |
| [`format`](../stdlib/.wtf/plugins/format.wtf) | Gauges, elapsed clocks, travel durations and cache age | Shared by the other libraries |

`plans` now owns the plan definition, decision-cell and constraint inlays together;
these previously came from the `definitions`, `decisions` and `constraints`
adapters. `timers` owns timer definition/reference inlays and controls. Task inlays
also call the timer library when a task has `@timer(...)`.

To customize one, copy its provider into your workspace and edit it. Copy an
imported library too if you want to change its behavior. Dependency resolution
prefers workspace libraries and falls back to bundled libraries. Native typed
constructors use the embedded library versions to keep note value semantics
stable; a workspace replacement customizes the importing provider or expression.
This also lets user features reuse a library under a new ID without changing the
standard feature. For example, a note can evaluate
`import("format").clock(90m)` or `import("timer").create("stopwatch", [])`.

The `timers` catalog supplies occurrence anchors, raw state, original declaration
identity and optional hover text; `days` supplies parsed calendar parts, stop
ranges, detail lines and cached forecasts. These are data adapters, shared with
queries. Parser vocabulary and typed expression lowering remain native. A plan
still uses the same mixed-integer numerical solver; modules never reimplement its
numeric algorithm. Timer controls evaluate transitions at execution time and
preserve the original declaration through aliases.

Inlay modules may define `time_dependent(ctx)` to override conservative clock
refresh detection. Bundled timers stop refreshing when paused or finished;
itinerary refreshes run only for documents containing days. Diagnostic queries
include module diagnostics, while a module's `diagnostics` input contains only
native diagnostics to prevent recursion.

The language's execution and value limits apply to bundled features too. Oversized
inputs report module errors rather than emitting partial hints or edits. Baseline
fixtures cover the previous solver, timer and itinerary output; additional tests
cover source validation, replacement, integer-second precision and large inputs.
