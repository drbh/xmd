# Queries and document inspection

Queries use the functional language from notes and modules: the same lexer,
parser, records, lists, lambdas, typed values, builtins, imports and evaluator.
Workspace collections are input bindings, loaded when an expression reads them.

```sh
wtf query trip.wtf 'map(filter(tasks, fn(t) => !t.done), fn(t) => t.title)' --json
wtf query trip.wtf 'sum(tasks.estimate)' --json
wtf query --workspace '{tasks: length(tasks), errors: length(diagnostics)}' --json
wtf ast trip.wtf
wtf graph trip.wtf
```

`query FILE QUERY` restricts input records to the specified note. Paths are relative
to `--root` (default: the current directory), or absolute. The CLI reads that file
and its explicit imports, including hidden, ignored, and external files. Names resolve within their own note; another file cannot
supply an undeclared name. Missing files produce an error.
Use `query --workspace QUERY` to query the whole workspace. The file is required
unless `--workspace` is set; the scope never depends on whether an argument looks
like a filename. The former `--in` option is no longer accepted.

Cross-note values use ordinary imports with a literal path:

```wtf
source := import("./values.wtf")
total := source.price * 2
```

Paths are relative to the importing note, including inside imported functions.
Members are evaluated on demand, and definition, rename, hover, and dependency
navigation retain their original source. Module IDs such as `import("agenda")`
continue to select library modules. File imports in query expressions are also
explicit: `wtf query trip.wtf 'import("./values.wtf").price' --json`.
A workspace expression resolves paths relative to `--root`; a pipeline row uses
its source note, and aggregate rows use the query scope. Missing imports in
unevaluated branches do not fail the query.

Workspace libraries and features activate only through the explicit
[modules manifest](../../stdlib/README.md#loading-and-replacement).

`ast FILE` and `graph FILE` are shortcuts for `query FILE ast --json` and
`query FILE graph --json`. Both accept `--query EXPRESSION`, `--root`,
`--jsonl`, clock options and `--fail-on-match`.

```sh
wtf ast trip.wtf --query 'filter(ast, fn(n) => n.kind == "definition")'
wtf ast trip.wtf --query 'ast | where kind == "cell" | select {text, source}'
wtf graph trip.wtf --query 'graph.nodes | where external | select {name, source}'
wtf graph trip.wtf --query 'graph.edges'
```

All queries are read-only. `capture` and `complete` have been removed from the
CLI; a generic mutation interface is future work. Editor task controls remain
available.

## Functional expressions

Collections include `ast`, `tasks`, `events`, `stops`, `entries`, `values`,
`plans`, `decisions`, `tables`, `rows`, `resources`, `diagnostics`, `notes`,
`sections`, `calculations`, `references`, `cells`, `days`, `timers` and `links`.
`graph` is a record with `nodes` and `edges` lists.

```wtf
map(filter(values, fn(v) => v.type == "Money"), fn(v) => {
  name: v.name,
  amount: v.value.amount,
  source: v.source
})
```

Use `get(record, "field")` or `get(list, index)` for optional access; a missing
entry returns `null`. Ordinary `record.field` access reports unknown fields.
`list.field` projects a field from every item. Records support shorthand
`{name, source}` wherever those names are in scope, including note functions.

| Operation | Expression |
| --- | --- |
| Filter | `filter(tasks, fn(t) => !t.done)` |
| Project | `map(tasks, fn(t) => {title: t.title, due: t.due})` |
| Count | `length(tasks)` |
| Aggregate | `sum(tasks.estimate)` |
| Reduce | `fold(tasks, 0, fn(n, t) => n + if(t.done, 1, 0))` |
| Sort | `sort_by(tasks, fn(t) => t.due)` |
| Group | `group_by(tasks, fn(t) => t.source.path)` |
| Limit | `slice(tasks, 0, 10)` |
| Reuse a note function | `map(tasks, summarize)` |
| Reuse a module | `import("format")` |

`sort_by` is stable and ascending, with nulls last. `group_by` returns
`{key, rows}` records in first-seen order. Both require scalar keys. `sum`
retains units, skips nulls and returns null for an empty input. Different
currencies require explicit conversion. `sum(table, row_expression)` remains
available in the shared language.

Predicates require booleans. Comparisons with null return false, except explicit
`==`/`!=` tests. `if`, `coalesce`, `&&` and `||` evaluate only needed branches.
`date(timestamp)` uses the request's timezone. `text(null)` remains null.
Functions retain lexical scope; query row fields never leak into named note or
module functions. `eval("expression")` resolves in the current document. For
file-specific functions and values, supply the positional note file.

The evaluator's existing limits apply, including expression depth, function call
depth, evaluation steps and collection growth. Dynamic expression evaluation is
also depth-limited. Errors go to stderr in the CLI, leaving stdout for results.

## Syntax data

`ast` is a flat list of linked nodes, built from the document parser and shared
expression AST without evaluating definitions. Malformed expressions remain
inspectable as `parse_error` nodes. A document root carries `schemaVersion: 1`.

Every node has:

- `id`, `kind`, and `name` (null when unnamed).
- `parent` (an ID or null) and `children` (IDs in structural order).
- `text`, the exact source slice, and `source`, its path, URI, one-based line and
  zero-based UTF-16 range, using the editor's location format.

Kinds cover documents, raw lines, sections, definitions, tasks, checkboxes,
attributes, events, tables, columns, rows, cells, plans, objectives, constraints,
itinerary days/stops/details, calculations, references, links and parser problems.
Expression nodes include literals, names, calls, operators, property accesses,
lists, records, lambdas and function applications. Their `role` identifies an
operand, field, argument or body; lambdas expose `parameters`.

Raw line nodes preserve prose, comments, fenced code and whitespace. Concatenating
those nodes' `text` reproduces the original file, including line endings. Semantic
nodes and raw lines are overlapping views, not disjoint edit ranges. IDs identify
nodes within the current snapshot; re-query after a document changes.

## Dependency data

`graph` is `{schemaVersion: 1, nodes, edges}`. It uses the editor's call hierarchy
analysis, including function calls, calculated table cells, checklist membership
and cycles. It is a static dependency graph, so references in conditional branches
can appear even if that branch is not evaluated. Unresolved references are
available through `references` and `diagnostics`; they have no resolved edge.

Nodes contain `id`, `kind`, `name`, `symbol`, `source` and `external`. Edges contain
`from` (the reader), `to` (its dependency), and `reads` (source locations in the
reader). Every endpoint has a corresponding node. A file query includes its own
nodes and direct external dependencies, marked `external: true`; it does not
expand the entire external documents. Workspace graphs include all indexed nodes.

## Existing pipelines and transports

Pipeline stages `where`, `select`, `sort`, `limit`, `count`, `sum`, and `group`
remain supported. Their expressions use the same evaluator. A pipeline can start with a functional
expression, such as `filter(ast, fn(n) => n.kind == "definition") | count`.
Metadata-only pipeline projections retain lazy definition evaluation.

Use `-` as the query argument to read a multiline expression with `//` comments
from stdin. There is no separate query-file format or query-file flag. Reusable
functions belong in ordinary note definitions or modules.

```sh
printf 'length(tasks)' | wtf query trip.wtf - --json
printf 'length(tasks)' | wtf query --workspace - --json
```

The editor's agenda uses an ordinary library function. It accepts document
records and an explicit date window, preserving overdue and undated tasks:

```sh
wtf query --workspace 'import("agenda").between(entries, today(), today())'
wtf query trip.wtf 'import("agenda").between(entries, today(), today() + 6d)'
wtf query --workspace 'filter(tasks, fn(t) => t.leaf && !t.done)'
wtf query --workspace 'filter(diagnostics, fn(d) => d.severity == "error")' --fail-on-match
```

`--on DATE` or `--now TIMESTAMP` pins the clock. JSON output is always an array: a list expression
supplies its rows; a scalar or record becomes one row. `--jsonl` emits a row per
line. `--fail-on-match` returns status 1 when there are any result rows.

The browser `query` request and LSP `wtf/query` accept the same parameters:

```json
{"query":"map(tasks, fn(t) => t.title)","uri":"file:///workspace/trip.wtf","now":"2026-09-18T12:00:00Z"}
```

`uri` and `now` are optional. A URI selects a document, including its current
unsaved buffer. Omit it for a workspace query. Results retain `schemaVersion`,
`rows`, `now` and the document `versions` snapshot. Invalid or unknown URIs are
errors. Through the JS library:

```js
await workspace.query(uri, "query", {query: "graph.edges"});
await workspace.request("query", {query: "length(tasks)"});
```
