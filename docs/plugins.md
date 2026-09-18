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
Legacy GitHub cache entries remain readable and are isolated from plugin data.
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
do not change the active registry. Invalid reloads retain the complete previous
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
steps, expression depth and function call depth. The first version has no imports,
mutable variables, asynchronous expressions, or third-party language runtime.

A working workspace is in [`examples/plugins`](../examples/plugins/demo.wtf).
