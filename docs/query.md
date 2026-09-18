# Workspace queries

`wtf query` (or `wtf q`) reads the same parsed and evaluated workspace as the
language server. Pipelines select collections, filter records, compute values,
project fields, sort, and aggregate. Queries never modify notes or refresh caches.
The language uses WTF expressions and units; it is not a jq or LogQL implementation.

```sh
wtf query '@today | select {title, due, at, source}'
wtf query 'tasks | where leaf && !done && contains(tags, "errands")'
wtf query 'tasks | where due != null && due < today() | sort due'
wtf query 'values | where name == "budget" | select {value, source}' --json
wtf query 'plans | where name == "bakery" | select solution' --json
wtf query 'rows | where table == "groceries" | sum cells.price'
wtf query 'tasks | group source.path | select {path:key, tasks:length(rows), effort:sum(rows.estimate)}'
wtf query 'diagnostics | where severity == "error"' --fail-on-match
wtf query -f queries/week.wq --on 2026-09-16
printf 'tasks | count' | wtf query -f - --json
```

In this repository use `cargo run -- query ...` or `./target/debug/wtf query ...`;
on macOS `/usr/bin/wtf` is an unrelated program.

To print a whole note with its IDE inline values, use
[`wtf render FILE`](rendering.md). Queries select structured data; rendering
preserves the document layout and adds the language server's inlay labels.

## Collections and records

| Collection | Records and notable fields |
| --- | --- |
| `tasks` | All tasks, including parents and completed tasks: `title`, `name`, `checked`, effective `done`, `leaf`, `parent`, `tags`, `due`, `scheduled`, `at`, `at_date`, `estimate`, `blocked_by`, raw `attributes` |
| `events` | Appointments: `title`, `at`, `at_date` |
| `stops` | Itinerary stops: `title`, `at`, `at_date` |
| `days` | Parsed itinerary days: `value` contains calendar parts, stops/details, source ranges and cached forecast metadata |
| `timers` | Timer occurrences: `value` contains limit/elapsed/start/idle state; `origin` identifies the declaration; `definition`, `inlay` and `anchor` describe placement |
| `entries` | Leaf tasks, events, and stops, with common scheduling fields |
| `values` | Named definitions: `name`, source `expression`, evaluated `type` and `value`; includes tables, timers, and plans |
| `plans` | Plan definitions, with structured `solution` (also exposed as `value`), including columns, choice cells and constraint source anchors |
| `decisions` | Solved decision cells: typed `value`, owning `plan`, exact `anchor` |
| `tables` | Table definitions; `value` is an array of objects keyed by column name |
| `rows` | Evaluated table rows: `table`, `cells`, and the row's source location |
| `resources` | Written links and literal resource definitions: `target`, cached `metadata` (or null) |
| `diagnostics` | Strict, non-editing diagnostics: `severity`, `message`, `code` |
| `notes` | Documents: `title`, `text` |
| `links` | Parsed link occurrences: `url`, precise source range |
| `sections` | Headings: `level`, exclusive zero-based `end_line` |
| `calculations` | Inline calculations: `expression`, `bracketed`, evaluated `value`, `type`, `display` |
| `references` | Bracketed references: `name`, optional `property`, evaluated `value`, `type`, `display` |
| `cells` | Individual table cells: `table`, zero-based `row`, `column`, `computed`, `expression`, evaluated `value`, `type`, `display` |

Every original record has `kind`, `title`, `source`, `errors`, a zero-based `line`,
and an `anchor` for inlay placement in UTF-16 coordinates. These same records are
[plugin inputs](plugins.md). Definitions also expose `computed` and evaluated
`display`. `source` contains
an absolute `path`, a file `uri` with a line fragment, a **one-based** `line`, and
an LSP `range` with **zero-based UTF-16** coordinates. These locations describe the
queried snapshot; they are not durable IDs or authority to edit a later snapshot.
Task `parent` is the parent task's source location, or null. `checked` is the
written checkbox; `done` accounts for the task hierarchy.

The common scheduling fields on `entries`, `events`, and `stops` include null
`due`, `scheduled`, and `estimate`, empty `tags` and `blocked_by`, and false `done`
when those fields do not apply. Undated itinerary stops have a text `at` time
and null `at_date`. Dates for timestamped events are calculated in the query's
offset. A recurring task without a due date has an effective `due` of today,
preserving the agenda behavior; `attributes` retains the original source.

`union(tasks, events, stops)` concatenates collections in the written order; it
does not remove duplicates. Without `sort`, collections use document path order
and then source order within each kind. `entries` visits tasks, events, and stops
within each document. Sorting is stable and leaves nulls last in either direction.

Definitions are evaluated when `value`, `type`, `display`, `solution`, or `errors` is needed,
or when a full record is output. `plans | select {name}` does not solve plans.
Tables must be evaluated to enumerate `rows`. Task scheduling/dependency fields
are evaluated when the task collection is built. Each query shares one engine.

## Pipeline language

| Stage | Meaning |
| --- | --- |
| `where expression` | Keep records for which the expression returns true |
| `select {title, deadline:due}` | Project or compute an object; aliases must be identifiers |
| `select expression` | Output a scalar, array, or object |
| `sort due, title desc` | Sort by compatible scalar keys; ascending is the default |
| `limit 10` | Keep the first 10 results, including zero if requested |
| `count` | Produce one numeric result, including zero for an empty input |
| `sum estimate` | Produce one typed sum, ignoring nulls; empty/all-null input produces null |
| `group source.path` | Produce `{key, rows}` objects; group keys use deterministic JSON order |

Expressions support the existing WTF literals and operators: strings, numbers,
booleans, money, durations such as `90s` and `2w`, ISO dates/timestamps, arithmetic,
comparisons, `!`, `&&`, and `||`. Boolean operators short-circuit. Parentheses,
quoted pipes, and `||` do not split pipeline stages. Bare names refer to fields
of the current record; they never implicitly resolve note definitions. Use
`eval("remaining / budget")` to evaluate a WTF expression in that record's source
document. After grouping or counting, `eval` resolves from the workspace root.

A projection replaces its input. Select fields needed by subsequent stages.
Dot access reads nested fields; on an array it projects that field from each
item, e.g. `rows.estimate` after grouping. Unknown fields are errors when read;
use `get(attributes, "every")` for optional keys. Reading a property of null
returns null. `null == null` is true; ordering comparisons with null are false.
Predicates require booleans, not truthy strings or numbers.

Functions:

- `today()` and `now()` use the query's frozen clock.
- `date("next Friday")`, `date("2026-09-18")`, and `date(timestamp)` produce dates;
  timestamp text accepted by WTF is also supported, retaining its datetime type.
- `contains(array, value)` tests membership; `contains(text, substring)` tests text.
- `length(array)`, `length(object)`, and `length(text)` count items, keys, or Unicode characters.
- `coalesce(a, b, ...)` returns the first non-null value without evaluating later arguments.
- `sum(array)` sums compatible numbers, durations, or money; mixed currencies fail.
- `get(object, "key")` returns null for an absent key.
- `text(value)` uses the human-readable representation; null stays null.
- `eval("expression")` reads note values through the existing WTF evaluator.

Queries are limited to 64 KiB and 128 pipeline stages. Individual expressions
have bounded token counts and depth. This version has no joins, arbitrary user
functions, regex operators, or persistent query index. Workspaces are scanned
for CLI invocations; filters and projections run in memory.

## Output, errors, and time

`--json` emits one array. `--jsonl` emits one JSON value per result line. Text
output prints scalars directly and object fields as tab-separated `key=value`
pairs. No results produce `[]` in JSON mode and no output in the other modes.
Use `select` to choose concise output. Full jq can process the JSON:

```sh
wtf query 'tasks | select {title, source}' --json | jq '.[].title'
```

JSON numbers, booleans, text, arrays, and objects retain their normal shapes.
WTF scalar units are explicit:

```json
[
  {"type":"money","amount":12.5,"currency":"USD"},
  {"type":"duration","seconds":90},
  {"type":"date","value":"2026-09-18"},
  {"type":"datetime","value":"2026-09-18T09:00:00-04:00"},
  {"type":"ratio","value":0.25}
]
```

Typed fields support arithmetic before serialization. Their JSON components are
also accessible through dot notation, such as `estimate.seconds`, `value.amount`,
or `value.currency`. Plan solutions contain numeric/typed objectives, variables,
and constraint values; they are never converted to display strings for JSON.
Timer values contain `state`, typed `elapsed` and `limit`, and `started`.

A failed note field evaluation appears as null with explanatory `errors` on its
record; missing optional values are null with no error. Filtering or projection
can exclude these records or their errors. Query `diagnostics` or select `errors`
when auditing a workspace. Invalid syntax, unknown fields that are actually read,
and incompatible operations fail the query without partial stdout output.
There is no static schema validation of expressions against an empty collection.

`--fail-on-match` returns status 1 after emitting results if any result exists.
It counts result rows, so `count` always produces a match, even when its value
is zero. Without that flag, empty queries and reported note diagnostics are
successful. Query and I/O failures return 1; CLI argument errors return 2. Errors
and match summaries go to stderr. Use `@check --fail-on-match` for CI validation.

`--root PATH` selects the notes workspace, honoring its ignore rules. CLI queries
read saved files. `--on YYYY-MM-DD` freezes both `today()` and `now()` at local
midnight; it rejects ambiguous/nonexistent local midnight. `--now RFC3339` fixes
an instant and offset explicitly for reproducible queries across machines. The
offset is fixed throughout a query, including future itinerary stops; it does
not model future daylight-saving transitions. These clock flags are mutually
exclusive. Without either, a single local clock snapshot is taken per query.

## Saved views and migration

The built-in views are ordinary query files in [`queries/`](../queries/), embedded
in the binary. They work from any directory and accept further pipeline stages.
Use `-f FILE` for personal saved queries; `-f -` reads a query from stdin. File
paths are relative to the process working directory, independently of `--root`.

| Previous CLI | Replacement |
| --- | --- |
| `wtf today` | `wtf query @today` |
| `wtf agenda` | `wtf query @today` |
| `wtf agenda --week` | `wtf query @week` |
| `wtf tasks` | `wtf query @tasks` |
| `wtf tasks --all` | `wtf query 'tasks \| where leaf'` |
| `wtf tasks --tag errands` | `wtf query '@tasks \| where contains(tags, "errands")'` |
| `wtf check` | `wtf query @check --fail-on-match` |
| `wtf plan bakery --json` | `wtf query 'plans \| where name == "bakery" \| select solution' --json` |

These are breaking CLI changes; the old read commands have been removed.
`@tasks` selects unfinished leaves, while the `tasks` collection includes every
task. `@today` and `@week` preserve overdue/undated tasks and malformed appointment
visibility. `@week` covers today through six days later. `@check` selects errors
and excludes warnings. The editor's **Show today's agenda** uses `@today` too.

`capture`, `complete`, and `refresh` remain explicit operations. Completion still
checks blockers, handles descendants, and advances recurring tasks with history.
There is no query assignment operator or generic mutation API in this version.

## Shared Rust, LSP, and browser APIs

```rust
use wtf::query::{self, Query, QueryContext};
let compiled = Query::parse("@tasks | select {title, source}")?;
let result = query::execute(&workspace, &compiled, &QueryContext::new(now))?;
// result.rows contains typed QueryValue values; serialize with serde_json.
```

The native LSP advertises and implements the optional `wtf/query` request:

```json
{"jsonrpc":"2.0","id":1,"method":"wtf/query","params":{
  "query":"values | select {name, value}",
  "now":"2026-09-16T12:00:00-04:00"
}}
```

`now` is optional. The server rescans saved notes, overlays open buffers, and
returns `{schemaVersion: 1, now, rows, versions}`. `versions` maps open document
URIs to LSP versions. Query failures return JSON-RPC invalid-params errors.

The WASM `BrowserWorkspace.request("query", payload, now)` method takes
`{"query":"..."}` with no document URI and returns the same snapshot fields
inside its existing `{ok, result}` envelope. The browser worker accepts the
`query` method through its normal message interface. Its `versions` map contains
all currently loaded documents. Native and browser hosts execute the same
compiled query against their own workspace snapshots; neither fetches external
data during evaluation.
