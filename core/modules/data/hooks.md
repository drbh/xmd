The hooks, records and `step` loops a module meets: what the host hands each hook and each step and what must come back. `core/modules/src/hooks.rs` reads this file, and `hosts/cli/tests/reference.rs` lays it out in `book/reference/writing-modules.md` as `kinds.md` says. The records a query also reads, like `recognized`, are described with the collections in `core/values/data/collections.md`.

Each `#` part holds `##` entries. An entry is its heading, one line of prose and `- ` lines, one per field or parameter, each `name: shape and meaning`; a `?` after a name marks one that may be absent. A hook's heading names it and the kinds of module it is called on, and its `returns` line what comes back. A step protocol lists its `input`, `output` and `effects` (`kind request -> answer: what it does`) under `###` headings. Prose is never wrapped.

# records

## url

A link's address, split. Only http and https links reach a link module.

- raw: Text: the whole URL
- host: Text
- path: Text
- scheme: Text: `http` or `https`

## link context

What a link module sees of one link it matched.

- url: url
- native: Boolean: false in the browser, where a refresh cannot run a program
- cached: what `decode` last returned for this link, or null before any refresh. A cache entry older than link modules gives `title`, `state`, `merged`, `checks`, `review` and `fetched_at` instead
- fetched_at: DateTime of the cached data, or null

## feature context

What a feature module sees of the note a request is about. `range` and `capabilities` are absent, not null, when a hook has no use for them.

- today: Date: the request's day
- midnight: DateTime: the start of `today` at the request's offset, the reference `at_time` places a day's times with
- document: Record: `path`, `uri` and `text` as text, `lines` as a list of text, and one list per collection the module's `inputs` names, under the collection's name. `inputs` defaults to sections, tasks, values and links (tasks, the bundled `tasks` module's, is empty while no active module declares it); `inputs: {tasks: ["text", "line"]}` keeps only those fields. With `values`, the same list is also `definitions`, each record with its first error as `error`. `recognized` holds only the module's own recognizers' matches, and like any collection is there when `inputs` names it. A collection a module declares in `collections` is named like any other, by any module
- module: Record: `id` and `revision` of the module being called
- range?: the LSP range being drawn, `{start, end}` of `{line, character}`
- capabilities?: Record: `refresh` and `views`, booleans for whether the host can refresh data and show views
- position?: the LSP position being completed or hovered, or where a `|` was just typed when formatting, `{line, character}`

## collection

One entry of a feature module's `collections` record, under the collection's name: a collection the module builds in its `records` hook, which queries bind and any module's `inputs` may name like the language's own. The name is an identifier no native collection and no other active module has. A module declares at most 16.

- entries?: Boolean: whether its records join `entries`, the collection a query lays on a timeline, beside tasks, events and stops; or Text, the name of a Boolean field, so only the records where it is true join (the `tasks` module's leaf tasks). False when absent
- from?: the collections only the `records` hook reads to build it, named like `inputs` (a list, or a record of the fields each keeps): the module's other hooks are not handed them, so a hook that runs on every edit reads the small built collection instead. Where `inputs` names one too, `records` reads it as `from` does. The `timers` module builds its timers from `values` and `mentions`

## built record

One record `records` returns. The host keeps it as it is and fills what it leaves out, and a query or module reads it like a native record. A collection holds at most 4096.

- line: Number: the zero-based line it is about. Required
- kind?: Text: the collection's name when absent
- title?: Text: the line's text when absent
- source?: Record: `path`, `uri`, `line` (one-based) and `range`, the whole line's when absent
- anchor?: the LSP position at the line's end when absent
- errors?: List of Text: empty when absent
- …: anything else. A record anywhere inside with a `lookup` field, `{kind, key, label?}` as `cached(kind, key, label)` takes them (the prelude's `forecast_lookup(place, date)` builds a day's forecast), asks for a cached lookup: the host puts in its place its other fields with `display` (the value as the prelude's `lookup_display` words it, or why it cannot be read), `source` and `fetched_at`, or null when nothing is cached. A refresh fetches every lookup asked for, and the record's line offers one

## recognizer

One entry of a feature module's `recognizes` list: a pattern the host runs over every line of one kind of block whenever a note is parsed, with no module code evaluated. Each non-empty match becomes a `recognized` record, unless the rule only paints. Patterns, groups and paints are checked when the module compiles: a bad one is a module error. A module declares at most 16. The bundled `itinerary` module declares the itinerary this way, and `tasks` how a task's checkbox is painted.

- name: Text: an identifier, once per module; each match's `recognizer`
- on: `prose`, `item`, `heading`, `row` or `line`: which lines it reads, from where their text starts (past a heading's `#`s, past a list marker and any checkbox, at a row's first `|`, past prose's indentation; a `line` rule reads any of those lines whole). `^` anchors there. Fences and comments are never read. A `prose`, `item`, `heading` or `row` rule finds every match on a line; of one module's `line` rules, the first that matches claims the line, once
- pattern: Text: a regular expression, at most 4096 bytes, with named groups `(?<name>...)`. Matching is linear in the line
- unless?: Text: a pattern; a line it matches is not this rule's. It says what a lookahead would, which patterns do not have
- under?: Text, `line` rules only: the name of another `line` rule with `until`. A line is this rule's only while a match of that rule is open, and its match belongs to the nearest one: its record's `parent`
- until?: `heading` or `break`, `line` rules only: how long a match stays open to the matches under it. `heading`: until the next heading. `break`: until the first line that is blank or that none of them claims. A match also closes when another match claims a line under the same parent, or above
- terms?: Record: a named group's terms, `[[text, term], ...]` in order (or a record of text to term), all text. A group whose captured text is one of them, ignoring case, has that `term`
- tokens?: Record: a named group's paint, one of `keyword`, `number`, `string`, `variable`, `heading`, `function`, `property`, `decorator`, `operator`, `comment`, `punctuation`, `money`, `date`, `time`, `duration`, `boolean`, `link`, `code`, `key` (the key of a `Key: value` line), `toggle`, `toggle_on` and `toggle_mixed` (the state of a control a line carries, off, on and mixed, as a tri-state checkbox shows it: a host may make it clickable, running the line's row action), `finished` (the text of something done, struck through) or `category1` to `category10` (a categorical palette: a module that needs distinguishable hues picks categories, and the theme colors them); or `{paint?, terms?, paints?, declaration?}`: the paint of the term of the first of `terms` (group names) that has one, else `paint`, marked as a declaration when `declaration` is true. `paints` gives terms their paints, `[[term, paint], ...]` (or a record of term to paint); a term it does not list paints as the paint it names, ignoring case. The note's own structure (links, names, attributes, comments) paints over it
- links?: Record: a named group's link, a URL whose `{}` is the captured text, form-encoded. Each becomes one of the note's links
- title?: Boolean: whether it reads a line only up to where its title ends, at its first attribute or a heading's or checklist item's trailing `:name`, so those paint as themselves. False when absent
- record?: Boolean: false when its matches only paint, and are no `recognized` records (a rule that only paints has no `under` or `until`). True when absent

## attribute

One entry of a feature module's `attributes` record, under the key a note writes after `@`: an attribute the module owns, which no other active module has. The language owns none. Notes are parsed knowing it, so its value is painted, checked and completed as what it holds, and the host evaluates it in the note's scope, wherever it is live, into the `attributed` collection: the module reads values, never note code. A value that fails is an `attribute` error on it (a `dependency` error for dependencies, with the items a cycle walks). A module declares at most 16. The bundled `tasks` module declares a task's attributes this way, and `appointments` an appointment's `@at`.

- value: `when` (a date or time: relative text such as `tomorrow`, or an expression that evaluates to one), `date` (a calendar date written `YYYY-MM-DD`, never evaluated, as an editor action stamps it), `duration` (an expression that evaluates to a nonnegative duration), `dependencies` (comma-separated conditions, each a Boolean or a checklist: the value is the ones not met yet, as `{text, name, source}`; a condition that names a checklist item brings that item's own dependencies in, and a cycle is an error), `expression` (any value), `text` (never evaluated), or `{tagged: [kinds]}` (the bare name of a definition whose own call makes a tagged record of one of those kinds, which claims it as its `origin`; the value is the record)
- params?: List of Text: its parameters, as signature help shows them
- applies?: Text: which lines take it, as signature help words it
- doc?: Text: what it means, for signature help, completion and the reference
- example?: Text: the value signature help and completion fill in
- values?: List of Text: the values completion offers inside it
- on?: `checkbox` or `any`: `checkbox` makes it live only on a checklist item (a list item with a checkbox), and prose anywhere else. `any` when absent

## form

One entry of a feature module's `forms` record, under the name a definition calls: a form the module owns, which no other active module has and no built-in is named. The language owns none. A definition whose whole expression calls it, `bakery := maximize(objective)`, is the module's to evaluate: notes are parsed knowing it, so the table under such a definition is the form's when it takes one, and the host reads its arguments and cells in the note's scope as the declaration says, then hands the module what they are worth in its `define` hook: the module reads values, never note code. Elsewhere in an expression the form is an error. A module declares at most 16. The bundled `plans` module declares `maximize`, `minimize` and `solve` this way.

- params: List of Text: its parameters, as signature help shows them, `name: what it holds`
- reads: List: how the host reads each argument, one per parameter: `linear` (a linear form over the form's unknowns) or `constraint` (`a <= b`, `a >= b` or `a == b`, each side a linear form)
- table?: List of `{name, reads, example?}`: the table under the definition, by column. `reads` is `name` (a cell that names its row: an identifier, once per table) or as for an argument; `example` is what the problem of a missing cell suggests. No table when absent
- unknowns: `free` (the names its expressions read that its note leaves undefined: each is a name of the note, which reads as the field of that name of the definition's value, so `bagels` reads `bakery.bagels`) or `own` (the definition's own name, which its other definitions read through as they are, so `monthly` is found inside `saved_by_june := monthly * 9`)
- unknown?: `{name, doc?}`: what a free unknown is called in the outline, and the words its hover adds after naming the definition that chooses it
- noun?: Text: what a definition of it is called in its table's problems; the form's name when absent
- returns?: Text: what a definition of it evaluates to, as signature help says it
- doc?: Text: what it means, for signature help, completion and the reference
- example?: Text: the arguments signature help and completion fill in

## formed

A definition that calls a form, as its module's `define` hook is handed it: every expression already read in the note's scope. A linear form is `{constant, terms, unit, per}`: `terms` the coefficient of each unknown by name, `unit` one of what the form counts in (`1`, `$1` or `1s`; `1` while it is only unknowns), and `per` one of what an unknown counts in, or null when its terms are scaled by two different units. A reading is `{text, range, anchor}`, the expression as written, its LSP range and the position at the end of its line, with a linear form's fields for `linear`, and for `constraint` `op` (`<=`, `>=` or `==`), `lhs` and `rhs` as linear forms and `difference`, their difference as one, or null when the two sides scale their unknowns differently. A failure reading one fails the definition, at the expression that failed, before the hook is called.

- form: Text: the form's name
- name: Text: the definition's name
- document: Text: the note's URI
- line: Number: the zero-based line of the definition's name
- arguments: List: each argument, read as the form says
- rows: List of `{line, cells}`: each row of the table that has a cell for every column, with `cells` read as the form says, in column order. Empty for a form without a table
- unknowns: List of Text: the names it solves for, first read first: the free names its note leaves undefined, or its own name
- decisions: List: the decision cells (of a `name?` or `name#` column) the sums its expressions read walk over, each an unknown of its own, `{name, domain, table, column, row, label, document, source, line, range, anchor, width}`: `name` the unknown's (`gear.take[1]`), `domain` `choice` (yes or no) or `count` (a whole number), the table's and column's names, the row's index from 0, `label` the row's first cell, the cell's note's URI, its text as written, its line, its LSP range out to the pipes, the position past its text and how wide it is between the pipes' padding

## action

What a control does, a record with a `kind`. A `document` is a note's URI, and `expected` is its whole text when the action was offered, or for a `row`, that row's: the action is refused if it changed since.

- invoke: `{document, expected, module, revision, event}`: call this module's `reduce` with `event` when the person runs it
- edit: `{document, expected, edits}`: apply LSP text edits
- row: `{document, row, expected, module, event}`: the row's own control, what clicking its checkbox does: call this module's `reduce` with `event` when the person runs it, against the note and clock of that moment. Refused only when the row no longer reads as `expected`, so an edit elsewhere leaves it standing. It leads its row's controls, and a host that prefers edits resolves it into one up front
- open_resource: `{target: {document, row, expected}, url}`: open a link
- refresh_resource: `{target: {document, row, expected}, url}`: refresh a link's data; needs the `refresh` capability
- refresh: `{document?}`: refresh lookups; needs the `refresh` capability
- show_today: `{}`: show the today view; needs the `views` capability

# hooks

## collect: feature

- ctx: feature context

returns List of inlays: `{at: position, label: Text, tooltip?: Text}` or `{line: Number, label, tooltip?}`

The inline labels for the note, with `ctx.range` set. `at` is an LSP position; `line` puts the label at that line's end. Every position is checked against the text. A failure shows one `module error` label on the first line. Required unless the module defines another feature hook: `actions`, `hovers`, `diagnostics`, `format`, `records`, `symbols` or `completions`.

## inlay: link

- ctx: link context

returns Text

The label shown after a link the module matched. A failure shows `module error · ...` as the label. Required.

## hover: link

- ctx: link context

returns Text

Markdown added to the link's hover. A failure shows its message instead.

## property: link

- ctx: link context
- name: Text

returns any value

What `link.name` reads. Called only for a name the module declares in `properties` and `property_names` allows for this URL. Required when `properties` is declared.

## refresh: link

- url: url

returns `{program: Text, args: List of Text, title?: Text, env?: Record of Text, format?: "json" or "feed"}`

The program a refresh of this URL runs. It is data: only a native host runs it, when a person asks for a refresh. A `./` or `../` program is relative to the module's file. A reply that does not fit, or a failure, means the link has no refresh. It sees a fixed clock, not the request's. Defined together with `decode` or not at all.

## decode: link

- url: url
- data: JSON value

returns Record

Turns the program's output into what is cached for the link, which later hooks see as `ctx.cached`. With `format: "feed"` the host parses the RSS or Atom document into JSON first. Anything but a record fails the refresh.

## matches: link

- url: url

returns Boolean

Whether the module takes a URL its `hosts` and `path_prefix` already admit. Anything but true, a failure included, is no. Required when the module declares no hosts. It sees a fixed clock, not the request's.

## property_names: link

- url: url

returns List of Text

Which declared properties this URL has. Names not in `properties` are dropped, and a failure means none. Without it every declared property applies. It sees a fixed clock, not the request's.

## time_dependent: link, feature

- ctx: link context or feature context

returns Boolean

Whether the output depends on the clock, so it is redrawn as time passes. A feature module gets `ctx.range`, and only true counts; for a link module anything but false counts, a failure included. Without it, a module is time dependent when it or a library it imports reads `now` or `today`.

## actions: feature

- ctx: feature context

returns List of `{line: Number, title: Text, action: action, disabled?: Text}`

The controls the note's lines offer, called once for the whole note with `ctx.capabilities` set; `line` is the zero-based line a control shows on. An action the host cannot perform is dropped, and the rest are checked before they show: one that fails hides the other controls on its line. A control with `disabled` says why it cannot run now: it is no lens or command, and a host that resolves row actions into edits shows it disabled with that reason. A failure, or a line outside the note, shows none.

## reduce: feature

- ctx: feature context
- event: any value

returns action, not `invoke` or `row`

The action an `invoke` or `row` control performs, decided when the person runs it, from the `event` the control carried. `ctx.capabilities` is set. For an `invoke`, the note and the module must still be at the text and revision the control was offered with; for a `row`, only its row must read as it did. A reducer that refuses with `error(reason)` has the person read the reason as written.

## hovers: feature

- ctx: feature context

returns List of `{range, contents: Text, fallback?: Boolean}`

Markdown hovers over LSP ranges of the note, with `ctx.position` the position hovered. The first module with one covering the cursor wins over the editor's own hover; one with `fallback` is shown only where the editor's own finds nothing more specific than the row. A failure shows none.

## diagnostics: feature

- ctx: feature context

returns List of LSP diagnostics: `{range, message, severity?, code?, source?}`

Problems shown with the editor's own. `source` defaults to `xmd`. A failure becomes one error diagnostic naming the module.

## format: feature

- ctx: feature context

returns List of LSP text edits: `{range, newText}`

Edits made when the note is formatted, every feature module's together: the bundled `tables` module's lay tables out. All of them have to apply together, or formatting fails. Typing a `|` in a table formats it too, with `ctx.position` just after the pipe: the editor keeps only the edits on that table's lines, and leaves the row being typed alone until it has its closing pipe and a cell for every column.

## records: feature

- ctx: feature context

returns Record: a list of built records under each collection the module declares in `collections`

Builds the module's collections for one note, once per revision of the note and its workspace and per day: queries, every module's `ctx.document` and the host read what it returned. It runs without the clock, so `ctx.today` and `ctx.midnight` are its dates and `now()` or `today()` fails. `ctx.document` holds the module's `inputs` but the collections modules build, `entries` included, and what its collections are built `from`. A failure leaves the collections empty and becomes one error diagnostic naming the module, on the first line it recognized. Required when the module declares `collections`.

## symbols: feature

- ctx: feature context

returns List of `{name: Text, detail?: Text, kind?: Text, line: Number, end_line?: Number, selection: range}`

Entries for the note's outline. One spans from `line` to its last filled line before `end_line` (one past `line` when absent) and nests by that span with the editor's own; `kind` is an LSP symbol kind in snake case, `namespace` when absent. One on a heading's line gives that heading's entry its detail and span instead. A failure adds none.

## completions: feature

- ctx: feature context

returns Null, or a list of `{label: Text, insert?: Text, detail?: Text, kind?: Text}`

What to offer at `ctx.position`, replacing the word being typed with `insert` (the label when absent); `kind` is an LSP completion kind in snake case. The first module with a list answers, even an empty one; null leaves the position to the editor. A failure is null.

## define: feature

- form: formed

returns `{value, hover?, detail?, record?}`

What a definition that calls one of the module's `forms` is worth, called while the note evaluates, at its clock. `value` is what the definition evaluates to; `hover` Markdown its hover adds after the calculation worked through; `detail` the one line the outline, the call hierarchy and completion show for it instead of its type and display; `record` what its records' `record` field holds, for queries and modules to read. A failure is the definition's error, at its first argument. Required when the module declares `forms`.

## step: command, provider

- ctx: step input

returns step output

One step of the loop a command or provider runs. Required. Its input, output and effects are the step protocol of the module's kind.

# steps

## command

`xmd run` calls `step` until it says `done`, performing the requests of each step in order between calls. Paths are relative to the directory the command runs in and cannot leave it. The clock stays where the run, or a repeat, started. A malformed request or an unknown kind stops the run.

### input

- args: Record: `flags`, from `--name value`, `--name=value` and bare `--flag` (true), with `-` in names read as `_`; `positional`, the other words as a list of text
- dir: Text: the name of the directory the command runs in
- state: what the previous step returned as `state`; null at first
- results: List: `results[i]` answers the previous step's `requests[i]`; empty at first

### output

- state?: any value, handed to the next step; null when absent
- requests?: List of effects to perform before the next step
- report?: List: each item is printed as a line
- done?: Boolean: true ends the run once `requests` are performed
- error?: any value but null: the run stops with it as the message, after `report` is printed
- repeat_after?: seconds, a number or text, read when `done`: wait (a day at most), then start over with null state

### effects

- http `{method?, url, headers?, json?, pick?}` -> `{status, json, text}`: An http or https request, GET unless `method` says PUT, POST, PATCH or DELETE. `json` is sent as the body. `json` in the answer is the parsed reply (null when it is not JSON, and then `text` has it). `pick` keeps only the named fields of each record in a JSON list reply.
- read `{path, json?}` -> `{text}`, or `{json}` when `json` is true: Read a file, parsed when `json` is true.
- list `{path}` -> `{files}`: The names of the files in a directory, sorted.
- write `{path, text}` or `{path, json}` -> `{}`: Write a file, `json` as pretty JSON, making its directory.
- move `{from, to}` -> `{}`: Rename a file, making the directory it moves into.
- remove `{path}` -> `{}`: Delete a file.
- credential `{scope, set?}` -> `{value}`: A secret saved for this command and scope in the user's config directory, stored first when `set` is given. `value` is null when nothing is saved.
- env `{name}` -> `{value}`: An environment variable, null when unset or empty. Only `XMD_*` names; any other stops the run.
- uuid `{}` -> `{value}`: A random identifier.

## provider

A refresh calls `step` for each lookup the notes want that no command in `.xmd/providers.json` answers, using the first provider that `provides` its kind. The loop is the command's, with a lookup instead of arguments and a value instead of a report.

### input

- key: Record: the lookup's `kind` and the parts of its key by name, as `cached(kind, key)` asked for it: `from` and `to` for a rate, `symbol` for a quote, `place` and `date` (a Date) for a forecast
- today: Date: the refresh's day
- state: what the previous step returned as `state`; null at first
- results: List: `results[i]` answers the previous step's `requests[i]`; empty at first

### output

- state?: any value, handed to the next step; null when absent
- requests?: List of effects to perform before the next step; only `http`
- done?: Boolean: true ends the loop, without performing `requests`
- value?: read when `done`: the lookup's value as JSON, every number stored as a decimal
- source?: Text: where the value came from; the module id by default
- error?: any value but null: this lookup fails with it as the message

### effects

- http `{url}` -> `{json}`: A GET request. A reply that is not JSON is a failed request. Any other kind of request fails the lookup.
