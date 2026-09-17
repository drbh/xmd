# Jot

Plain-text notes with calculated values, checklists, timers, reusable links, and dates.
The same Rust engine powers the language server and terminal commands.
Use it in **Zed, VS Code, Neovim, or Helix**; see [editor setup](docs/editors.md)
for the extension/configuration and a shared smoke test.

Jot is **LSP-first**: language/editor features belong in the shared Rust core and
native language server first. Browser clients consume the same LSP data and
operations; they must not introduce browser-only language behavior.

There is also a **browser-only, dark-mode editor**: vanilla HTML/JavaScript,
CDN-loaded Monaco, and the same Rust engine compiled to WebAssembly in a worker.
No native Jot process or language-server backend is needed. See [browser setup](web/README.md).

```sh
bash web/build.sh
node web/serve.mjs
# Open http://127.0.0.1:4173
```

## Try it

```sh
cargo build
./target/debug/jot today
./target/debug/jot agenda --week
./target/debug/jot tasks --tag errands --json
./target/debug/jot check
./target/debug/jot plan bakery
```

Open `jots/interactions.jot`, `jots/daily.jot`, or `jots/timers.jot` in Zed. If you already installed the dev
extension, restart the Jot language server after rebuilding the binary. The
extension uses `target/debug/jot` in the project root. To install it initially,
use **Install Dev Extension** and choose `zed-extension`.

For notes in other projects, set `lsp.jot.binary.path` in your Zed user settings
to this repository's absolute `target/debug/jot` path and `arguments` to `["lsp"]`.
The extension honors that override and otherwise uses the project's debug build.

Enable CodeLens and automatic signature help in your Zed user settings, alongside
Jot's highlighting and inlay hints:

```json
{
  "code_lens": "on",
  "auto_signature_help": true,
  "languages": {
    "Jot": {
      "semantic_tokens": "full",
      "document_symbols": "on",
      "inlay_hints": { "enabled": true, "show_other_hints": true }
    }
  }
}
```

`code_lens` and `auto_signature_help` are top-level, editor-wide settings, not
Jot language overrides. CodeLens displays clickable labels above relevant lines.
Use `"code_lens": "menu"` if you prefer controls in the code actions menu.
See [Zed's settings reference](https://zed.dev/docs/reference/all-settings#code-lens).

`languages.Jot.document_symbols: "on"` makes Zed use Jot's LSP symbols for its
outline and breadcrumbs instead of tree-sitter. After restarting the language
server, open Zed's outline or Go to Symbol in Editor to navigate the note.

Rebuild the dev extension after changing its language configuration or token
rules. On macOS, `/usr/bin/jot` is an unrelated command; the examples deliberately
use `./target/debug/jot` or `cargo run -- ...`.

## Editor interactions

Three features ride on standard LSP requests that most editors already send:

- **Plain-text charts.** Inlay hints, hovers and tooltips draw with Unicode
  blocks, so they render in any editor without image support. Checklist
  headings, parent tasks and countdowns carry a live gauge in their inlay
  (`███░░░░░ 2/5 complete`, `⏳ ████░░░░ 12:00 remaining · running`); hovers
  add the percentage (`████░░░░░░ 40%`). Numeric table columns and `sum(...)`
  row contributions show a sparkline with their range (`▁█▅ 2 → 6`).
- **Format on type.** Typing the closing `|` of a table row realigns the whole
  table; the row you are typing is only padded once every column has a cell, and
  only its whitespace changes so the caret stays put. Enter after a checkbox
  continues the checklist with the same indent, and Enter on an empty checkbox
  ends it. Zed sends these automatically; the browser editor sets
  `formatOnType`.
- **Linear plans.** `[bakery] := maximize(3 * bagels + 1.25 * doughnuts)`
  followed by a `| constraint | expression |` table declares an optimization.
  Names no note defines are decision variables (never negative); every other
  name is a constant read from your notes, so plans re-solve as you edit. Money
  and durations are unit-checked. Inlays show the objective, each variable, and
  per-constraint usage with binding or slack; hovers add usage bars; infeasible
  or unbounded plans are diagnostics on the objective. `bakery.bagels` reads a
  variable and `bakery.flour` a constraint's slack. On the command line,
  `jot plan bakery` prints the solution, `--export` writes an
  [alps](https://github.com/drbh/alps) problem file, and `--import file.json`
  prints Jot source. The solver is pure Rust (`good_lp` with `microlp`), so it
  also runs in the browser. See `jots/plans.jot`.
- **Calculated cells.** A table cell in brackets is a calculation, just like
  `[cash]` in prose: `| bulk | [unit * qty] |` reads named values from any note
  and shows its result as an inlay. Columns keep one type, and a calculated
  cell of the wrong type is reported at that cell.
- **Goal seek.** `[monthly] := solve(saved_by_june >= $5,000)` makes the
  definition's own name the unknown and finds the boundary value through any
  chain of calculations, with the unit inferred from the chain. Linear
  equations have a closed form, so no solver runs.
- **Decision columns.** A table column named `take?` is a yes/no choice per row
  and `servings#` a whole number. A plan that sums over the column, such as
  `maximize(sum(gear, value * take))`, chooses every row: each cell gets an
  inlay with its choice, the plan hover lists what was picked, and a code
  action on the plan line writes the choices into the table. Outside a plan a
  decision column is not data.
- **Itineraries.** A line such as `Friday, November 20 · New York | Oaxaca`
  (with or without `##`) starts a day; a line starting with a time such as
  `07:04 AM` or `14:30` is a stop; `Key: value` lines beneath it are details
  and other lines are notes. Years carry forward, and a first day without one
  is the next occurrence. Format Document pads times and indents details.
  Stops paint their time and title, days show a stop count and how far away
  they are, each stop shows the time until the next, `Cancel by: 24h before`
  or a datetime shows the deadline, `Address:` lines open in Maps, the outline
  lists days and stops, days and stops fold, completion offers stop kinds
  after a time and detail keys inside a stop, and `jot agenda` includes stops.
  Diagnostics catch a weekday that does not match the date, days out of order,
  stops out of order, and impossible dates. See `jots/oaxaca.jot`.
- **Dependency graph.** `textDocument/prepareCallHierarchy` treats a value,
  column, task, or checklist as a node. *Incoming calls* list everything that
  reads it (calculations, `@after`, `@estimate`, parent tasks, checklists);
  *outgoing calls* list what it depends on. VS Code shows this as the Call
  Hierarchy tree; Zed does not expose call hierarchy yet.


Open `jots/interactions.jot` to try these without changing any language syntax:

- **Semantic highlighting.** Shared Rust tokens distinguish function calls, bold
  declarations, table columns, money, dates, durations, percentages, and metadata.
  Brackets and table separators stay subdued. Open checkboxes are bold amber;
  checked boxes are bright green, separate from muted, struck-through task text.
  Prose recognizes ISO dates, month/day/year dates (`09/17/2026`), clock times
  (`7AM`, `10:00 AM`, `14:30`), relative dates (`tomorrow`, `next Monday`), money,
  percentages, compact durations, numbers, and booleans. Dates/times are bold
  pink/cyan. This is highlighting only: prose does not create symbols or schedule
  tasks, and calculations still use ISO date syntax. Code/comments, links, and
  identifiers keep their own colors. The Zed extension supplies a
  Jot-only dark palette (without replacing your editor theme); the browser uses
  the same colors and fetches its token legend from Rust. Rebuild the dev extension
  and restart the Jot language server to load new token rules.
- **Document symbols.** Standard `textDocument/documentSymbol` supplies nested
  headings, tasks/subtasks, named literals, calculations, timers, and events.
  Definitions include their evaluated type/value in `detail`; task details show
  completion. Navigation selects the name, and enclosing ranges describe the
  section/task hierarchy. Clients without hierarchy support receive flat symbols.
  Values are snapshots when requested; LSP has no document-symbol refresh request.
- **Clickable controls.** CodeLens shows task completion/reopening, timer
  start/pause/resume/reset, resource opening, and explicit GitHub status refresh.
  Controls also appear in code actions on the relevant line. A stale task or
  resource control refuses to act if its source line has changed; request fresh
  actions if that happens. Timer state is captured when a control is executed.
- **Context-aware completion and signatures.** Names include their current type
  and value. `@timer(` offers named timers; `@due(` offers dates; `effort(` offers
  checklists. A dot offers properties for that value. Functions and metadata have
  argument snippets and signature help tracking the active argument, including
  nested calls. Clients without snippet support receive plain insertions.
- **Calculation explanations and navigation.** Prose references such as
  `[remaining]` show their current value immediately after the closing bracket,
  without changing the file. Hover a calculated name or its value inlay for the
  original expression, substituted values, and
  linked inputs. Timer hovers show state and timing; task hovers explain blockers
  with definition links. Document highlights distinguish declarations from reads;
  prepare-rename selects just the name, leaving brackets and properties intact.
- **Selection refactorings.** Select a literal in prose to extract `[value]:name`,
  or select a complete subexpression to extract a named calculation above it.
  Generated names are collision-free; use rename to give them a personal name.
  On a reference inside a calculation, **Inline expression** preserves precedence
  and refuses cross-file substitutions that would change which names resolve.
  **Freeze current value** explicitly snapshots a scalar reference (also in
  prose); it never runs automatically. Formula snapshots are only offered when
  the value can round-trip without changing its type or precision. Timers and
  resources are controlled/opened, not implicitly materialized.
- **Actionable diagnostics.** Errors point at offending operands; unknown names
  offer nearby-name corrections and an explicit TODO definition. Ambiguous names
  and dependency cycles link to relevant declarations. While editing incomplete
  expressions, their errors and dependent cascades are suppressed; `jot check`
  remains strict. Clock-dependent diagnostics update without typing. Timer
  controls refresh at expiry, and ticking never rewrites source.

These use standard LSP requests; no Jot-specific document viewer is required.
Editor support determines presentation. All source changes go through undoable
workspace edits, with document versions attached for open buffers. Opening or
hovering a GitHub resource never fetches metadata; refresh remains explicit.

## Checklists and calculations

```text
## Release :release
- [x] Write parser
- [ ] Review implementation :review @estimate(30m)
- [ ] Publish package @after(review) @estimate(10m)

[progress] := completed(release) / total(release)
[remaining_work] := effort(release)
```

Use Zed's code actions on a task to **Complete task** or **Reopen task**. After
rebuilding the dev extension, pressing Enter at the end of a checkbox line
continues the checklist on the next line. Completing a parent updates its
descendants. A parent's computed completion follows its
children; manually editing `[x]` remains possible. Actions refuse blocked tasks,
but there is no restriction on editing your own source text.

Headings display completion counts and estimated work left. Functions `total`,
`completed`, `remaining`, and `effort` accept a named heading. They count leaf
tasks under that heading (including child headings), without counting parents
twice. `effort` sums estimates only for unfinished leaves; an absent estimate
contributes zero. Empty-checklist division produces a diagnostic.

Names use ASCII letters, digits, and underscores, beginning with a letter or `_`.
A heading or task's optional `:name` goes after its title and before metadata.
Dependencies use `@after(review)` or `@after(review, other_task)`; a named
checklist or boolean expression can also be a dependency. Dependency cycles and
unknown names produce diagnostics.

Literals retain the original `[value]:name` syntax. Derived definitions use
`[name] := expression`. Multiple literals per prose line work. Arithmetic
supports precedence, parentheses, unary signs, comparisons, `&&`, `||`, and `!`.
Money retains its currency formatting; money/money and count/count ratios show
percentages. Ordinary numeric division stays numeric. Durations use `s`, `m`, `h`,
`d`, or `w` and are stored as whole seconds. Fractional units such as `0.5m` are
accepted when they resolve to whole seconds. Strings use double quotes inside
expressions. Markdown links, images, inline code, fenced code, and HTML comments
are distinguished from Jot names. Example syntax inside code/comments is inert.

## Computational tables

Open `jots/tables.jot` to try a table with named columns and row-wise sums:

```text
[groceries] := table
| item  | quantity | price |
|-------|----------|-------|
| apple | 2        | $3.30 |
| pear  | 4        | $4.30 |

[total] := sum(groceries, quantity * price)
[units] := sum(groceries, quantity)
[average_price] := total / units

Groceries will cost [total].
```

The total evaluates to `$23.80`. Adding a row includes it automatically; source
files never receive calculated results unless you explicitly apply a refactoring.

- Only `[name] := table` starts a computational table. Its header must be on the
  next line, followed by a Markdown separator row and contiguous data rows. Use
  outer `|` delimiters; a blank or non-table line ends the table. Ordinary Markdown
  tables and examples inside fenced code/comments remain non-computational.
- Column names are unique identifiers, excluding `true` and `false`. Cell values
  are **literals**, not formulas or references: numbers, money, ratios, durations,
  dates/timestamps, booleans, text, and resources. Bare words such as `apple` are
  text; quote numeric-looking text. Quoted pipes and escaped `\|` are supported.
  Missing cells, malformed rows, and type mismatches produce diagnostics.
- Each column's type is inferred from its first valid value; subsequent cells
  must have that same type. Money and ordinary numbers are distinct types.
- `sum(table, expression)` evaluates the expression for each row and adds the
  results. Names inside the row expression refer **only to that table's columns**,
  not same-named globals. The first argument is a table name or a named alias;
  tables and formulas may live in separate notes. Nested sums get independent
  row scopes. Results must be numbers, money, ratios, or durations. Summing an
  empty table reports an error because its result type cannot be inferred.
- Standard LSP supplies column completion and signature help, definition/references,
  scoped rename, cell/column hovers, row contributions in a direct sum's hover,
  semantic highlighting, diagnostics, and table/column outline symbols.
  **Format Document** aligns computational tables only, preserving literals,
  alignment markers, line endings, and surrounding prose. Malformed tables are
  left untouched. Refactors that would extract row-local formulas out of their
  scope are not offered. Very expensive nested calculations have a step limit.

These are shared engine/LSP features. Reload the browser after rebuilding Wasm
and import `jots/tables.jot` to use the same features there, including undoable
Format Document. Existing browser-saved notes are not replaced.

## Stopwatches and countdowns

```text
[focus] := countdown(25m)
[debugging] := stopwatch()

- [ ] Investigate flaky test @timer(debugging) @estimate(30m)

Time left: [focus.remaining].
Time spent: [debugging.elapsed].
[over_estimate] := debugging.elapsed > 30m
```

Timers start **idle**. Put the cursor on a declaration, a reference, or an
associated checklist line and open code actions. **Start timer**, **Pause timer**,
**Resume timer**, and **Reset timer** appear when applicable. Start/resume capture
the time when the action is executed. Reset returns to idle and clears elapsed
time; use the editor's undo to undo a timer action. Completing a checklist task
does not automatically stop its timer. `@timer` accepts a named timer, including
an alias or a unique definition in another note.

Running hints refresh approximately once per second in open notes. Zed controls
the final rendering cadence. Ticking never edits a file, republishes highlighting,
or sends refreshes once no open note depends on a running timer or `now()`.
Countdowns stop at zero: `.done` becomes true, `.running` becomes false, and
`.elapsed` is capped at the countdown duration. Expiry is visual for now; there
is no audible alarm, OS notification, or background service.

Timer properties are `.elapsed`, `.running`, `.done`, and `.state` (`"idle"`,
`"running"`, `"paused"`, `"done"`). Countdowns additionally expose `.remaining`
and `.duration`; a stopwatch's `.done` is always false. Duration properties work
in arithmetic and comparisons. `[name.property]` works in prose with inlays,
hover, completion, navigation, and rename of the underlying name.

Actions persist state in the expression itself; save the note to preserve it
across restarts. The optional arguments are elapsed duration, then the timestamp
at which the current running segment began:

```text
[focus] := countdown(25m, 0s, 2026-09-16T14:00:00-04:00)
[debugging] := stopwatch(73s)
```

Here the countdown is running (or done, depending on the current time), and the
stopwatch is paused at 73 seconds. With a timestamp, elapsed time is accumulated
time plus time since that timestamp; without one, elapsed time is frozen. This
keeps notes self-contained and makes sleep/closed-editor time count while a timer
is running. There is no hidden timer cache. Controls preserve the countdown's
original duration expression, even when it refers to another value.

`now()` provides a live timestamp, sampled once per evaluation. For example,
`[until_meeting] := meeting_at - now()` is a signed duration until an appointment.
Use timer actions to capture a start time; writing `stopwatch(0s, now())` would
continually move the start to the present. Timers use wall-clock timestamps, so
system clock adjustments affect them (negative running intervals clamp to zero).
They have whole-second resolution; pausing discards a fractional second and is
not intended for precision profiling.

## Reusable resources and workspace names

```text
[https://github.com/owner/repo/pull/42]:parser_pr
[./assets/receipt.jpg]:receipt
[geo:40.7306,-73.9866]:cafe

- [ ] Review [parser_pr]
- [ ] Expense lunch using [receipt]
Meet at [cafe].
```

Use hover for details and image previews where the editor supports images in
Markdown hovers; use document links to open files, URLs, or the map. Image bytes
stay in their original files. Relative paths are resolved against the defining
note, even when a reference is used in another folder. Normal Markdown links
and images are clickable too.

Raw resources work directly in prose, headings, tasks, and resource table cells:

```text
Review https://github.com/zed-industries/zed/pull/123
Read ../README.md and src/main.rs.
Settings: ~/.config/zed/settings.json
Map: geo:40.7306,-73.9866
```

These get semantic highlighting, standard LSP document links, resource hovers,
and Open resource actions—no brackets or named definitions needed. HTTP(S),
`file://`, `geo:`, absolute paths, `./`, `../`, and `~/` are supported, along with
relative paths containing a file extension and common filenames such as
`README.md`, `Cargo.toml`, and `.gitignore`. Use `./` for an ambiguous extensionless
path; use a Markdown link for paths containing spaces. Trailing sentence
punctuation is excluded, while balanced URL parentheses are preserved. Code and
comments stay inert. Relative paths are based on the containing note; `~/`
resolves to your home directory in the native editor.

The browser uses the same Rust document links and can navigate to imported
`.jot` files or open HTTP(S) URLs. It cannot read arbitrary local files or expand
your home directory. Try `jots/highlighting.jot` for the full palette and links.

The workspace indexes `.jot` files recursively, respects ignore files, and skips
hidden/build directories and symlinks. Open editor buffers override disk content.
Names resolve in the current file first, then to a unique workspace definition.
Ambiguous names are reported; there are no implicit namespace guesses. Completion,
go-to-definition, find references, and rename work across notes, including task
dependencies and date formulas. Duplicate target names are rejected by rename.

### GitHub status

Install/authenticate the GitHub CLI (`gh`), then run:

```sh
./target/debug/jot refresh
```

Or use **Refresh GitHub status** on a resource's CodeLens or code actions to
refresh just that resource. The CLI refreshes all indexed GitHub resources. This only reads
GitHub; it never merges PRs or changes issues. It supports GitHub.com PRs, issues,
and commits. PRs expose title, state, merged status, reviews, and check summaries.
Links show the cache timestamp. Failed refreshes preserve the previous cache;
missing checks are unknown, not passing. The ignored `.jot/cache.json` contains
cached metadata and timestamps, never credentials. Remote status does not update
until you explicitly refresh it again.

```text
[ready_to_ship] := parser_pr.merged && parser_pr.checks_passed
```

Properties include `.merged`, `.checks_passed`, `.state`, `.title`, `.url`, and
`.exists` (local files). Remote properties produce a diagnostic until their
metadata is available. A cached status is an observation, not a deployment gate.

## Dates, recurrence, and the agenda

```text
[2026-09-25]:departure
[days_left] := departure - today()

- [ ] Book hotel @due(departure - 7d)
- [ ] Renew prescription @scheduled(2026-09-17) @due(2026-09-20)
- [ ] Review finances @every(month) @due(2026-09-30)
- Meet at [cafe] @at(2026-09-18T14:00-04:00)
```

`@due` is a deadline; `@scheduled` is a planned work date; `@at` is an appointment.
Dates use ISO `YYYY-MM-DD`. Timestamps accept ISO times with an explicit offset,
or local `YYYY-MM-DDTHH:MM` (ambiguous daylight-saving times require an offset).
Date-only arithmetic requires whole days; timestamps can add hours, minutes, or seconds.
`today()` is deliberately dynamic and editor hints refresh at midnight.

Relative input supports `today`, `tomorrow`, `yesterday`, and `next Friday` (and
other weekdays). Capture converts these to fixed dates. When typing directly in
Zed, use **Resolve relative dates to calendar dates** to freeze them; until then,
relative text is evaluated against the current day. A weekday means its next
occurrence, strictly after today. `date("next Friday")` is an explicitly dynamic
expression.

Recurrence supports `day`, `week`, `month`, `year`, or whole-day durations such
as `2w`. Complete it through a code action or `jot complete`: the task remains
unchecked, its due date advances past the completed occurrence, and an HTML
`jot-history` comment records the title, completion date, original due date, and
next date. `@repeat_from` preserves the recurrence anchor, including month-end
behavior (January 31 → February 28 → March 31). Missed instances advance to the
next future occurrence rather than creating a backlog. Recurrence on parent tasks
is rejected; complete recurring leaves individually. Hand-editing `[x]` does not
run recurrence/history actions.

```sh
./target/debug/jot today
./target/debug/jot agenda --week
./target/debug/jot tasks --tag errands --json
./target/debug/jot tasks --all
./target/debug/jot capture "Call the dentist" --due "next Friday" --tag errands
./target/debug/jot capture "Review notes" --journal
./target/debug/jot complete inbox.jot:1
```

All commands accept `--root PATH`. Agenda/task commands also accept `--on
YYYY-MM-DD` for reproducible queries; capture/complete accept it for backdated
entries. Task tags use `#errands` or `@tag(errands)`.

Today includes unfinished tasks that are overdue, due, scheduled, or undated,
and today's appointments. The week view covers today through six days from now.
`tasks` lists all unfinished leaves, including future tasks; `--all` includes
completed leaves. Results contain source paths and one-based line numbers; JSON
includes URI, dates, tags, estimates, blocked reasons, and evaluation errors.
JSON includes exact `estimate_seconds` and `estimate_minutes` (which can be fractional).

**Show today's agenda** in Zed opens a generated `.jot/today.md` with links back
to source notes. Re-run the action to refresh the view. Capture appends to
`inbox.jot`, `--journal` uses `journal/YYYY-MM-DD.jot`, and `--file` chooses a note.
CLI commands read saved files, so save editor changes before completing tasks
from the terminal. Editor actions use versioned edits against the live buffer.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo build
```

With no subcommand (or `lsp`), Jot speaks standard LSP on stdio. Do not run that
process separately when using Zed; the editor starts and owns the connection.
