The collections a query or a feature module's `inputs` can name, and what each
field of their records means. `hosts/cli/tests/reference.rs` reads this file
into `book/reference/collections.md` and checks it against the records the
examples hold: every collection has an entry, every field a record has is
described, and nothing is described that no record has.

`# every record` holds the fields most records share, and `# fields` the ones
that mean the same in every collection that has them. Every other `#` part is a
group of collections, its first line the group's intro. Each `##` entry is a
collection: its name, one line of prose, an `example:` line (a shell command,
run in `examples/` with `xmd` on the path), and `- name: meaning` lines for the
fields it has that the two shared parts do not describe, or describe
differently. Prose is never wrapped.

# every record

- kind: Text: what the record is, the collection's name in the singular: `task`, `value`, `section`
- line: Number: the zero-based line it is on
- title: Text: a short label for it, as a list of records shows it
- source: Record: where it is: `path`, `uri`, `line` (one-based) and `range`
- anchor: the LSP position at the end of its line, where your editor draws its result
- errors: List of Text: what went wrong with it, empty when nothing did

# fields

- name: Text or Null: the name it was given, `budget` in `$3,000:budget`
- hover: Text: the Markdown your editor shows when you hover it
- value: what it evaluates to, typed: money, a date, a duration, a list; null when it fails
- display: Text: its value as the note shows it inline, `$1,324`
- type: Text: the kind of its value: `Money`, `Date`, `Table`, `Plan`
- expression: Text: the expression as written
- computed: Boolean: whether it is calculated, rather than written as a value
- bracketed: Boolean: whether it is written in square brackets in a sentence, `[total]`
- property: Text or Null: the field it reads, `remaining` in `focus.remaining`
- end_line: Number: one past its last line, the lines it holds included
- form: Text or Null: the form its definition calls, `maximize`, `minimize` or `solve`
- grid: List of `{line, cells, text}`: the table under it, each line as written, header and divider included
- header: Number or Null: the line of that table's header
- problems: List: what is wrong with that table, like a missing cell or a bad name
- record: Record or Null: everything a form worked out, for a definition that calls one
- done: Boolean: whether it is complete; an item with subitems is complete when every one is
- leaf: Boolean: whether it has no subtasks of its own. Only leaves join `entries`
- at: Date or DateTime: when it happens, from `@at` or an itinerary's time, or null
- at_date: Date: the day of `at`, or null
- due: Date or DateTime: from `@due`; a task with `@every` and no `@due` is due today
- scheduled: Date or DateTime: from `@scheduled`, or null
- estimate: Duration: from `@estimate`, or null
- tags: List of Text: its `#tags` and `@tag`s
- blocked_by: List of Text: what its `@after` still waits on, as written
- blockers: List of `{name, source}`: the named items its `@after` still waits on
- blocked_error: Text or Null: why its `@after` could not be read
- checked: Boolean: whether its own box is ticked, `[x]`. `done` also counts its subtasks
- in_progress: Boolean: started but not done: marked `[-]`, or some of its subtasks done
- attributes: Record: each attribute it writes, as written, by key: `{due: "2026-10-15"}`
- children: List of `{line, done}`: its subtasks
- parent: Record or Null: the item it nests under, as `path`, `uri`, `line` and `range`
- schedule: List of `{key, value, error}`: its `@due`, `@scheduled` and `@at`, in the order written
- timer: Timer or Null: the timer its `@timer` names
- table: Text: the name of its table
- schemaVersion: Number: the version of the format, which changes when a field does

# values and calculations

What a note defines and computes.

## values

Every definition in a note: a value with a name written in a sentence, `$3,000:budget`, or a calculation, `total := car + $90`.

example: xmd 01-values.x.md 'values | map(.{name, display})'

## calculations

Every expression a sentence puts in square brackets, `[total]` or `[each * 3]`.

example: xmd 01-values.x.md 'calculations | map(.{expression, display})'

## references

Every place an expression reads a name, with the value it reads there.

example: xmd 10-timers.x.md 'references | map(.{name, property, display})'

- name: Text: the name it reads
- hover: Text: the expression as written, which your editor's hover starts from

## mentions

Every place a note names something: each definition's name and each name it reads, in a sentence or in an expression.

example: xmd 01-values.x.md 'mentions | filter(fn(m) => m.name == "budget") | map(.{line, bracket})'

- name: Text: the name
- bracket: Boolean: whether it is in square brackets in a sentence

# tables and plans

Tables with a name, their rows and cells, and the plans that choose values for them.

## tables

Every table with a name: `groceries := table` and the table under it.

example: xmd 11-tables.x.md 'tables | map(.{name, display})'

- value: List of Record: its rows, each cell by its column's name

## rows

Every row of a table with a name.

example: xmd 11-tables.x.md 'rows | map(.cells)'

- cells: Record: the row's cells by column name, each evaluated

## cells

Every cell of a table with a name.

example: xmd 12-calculated-cells.x.md 'cells | filter(fn(c) => c.computed) | map(.{column, row, display})'

- column: Text: its column's header
- row: Number: its row, from 0
- computed: Boolean: whether it is a calculation in brackets, `[unit_price * 3]`

## plans

Every definition that calls `maximize` or `minimize`, with the values it chose.

example: xmd 14-plans.x.md 'plans | map(.{name, display})'

- value: Record: each unknown it solved for, by name, and `objective`, the best the objective can be
- solution: Record: the solved plan: each unknown, the objective, and each constraint and whether it binds

## forms

Every definition that calls a form a module declares, like `maximize`, `minimize` and `solve`.

example: xmd 16-goal-seek.x.md 'forms | map(.{name, form, display})'

## decisions

Every decision cell of a plan's table: one in a `name?` column (yes or no) or a `name#` column (how many).

example: xmd 15-decision-columns.x.md 'decisions | map(.{line, value})'

- title: Text: the name of the plan that decides it
- plan: Text: the name of the plan that decides it
- value: Boolean or Number: what the plan chose

# tasks and time

Checklists and tasks, and everything with a place in time.

## tasks

Every task: a checklist item, `- [ ] Pack`, with its attributes worked out.

example: xmd 06-task-attributes.x.md 'tasks | filter(fn(t) => !t.done) | map(.{title, due, blocked_by})'

## checkboxes

Every list item with a checkbox, as the language reads it before the `tasks` module makes tasks of them. A named item is a Boolean, whether it is done, and a named heading is the checklist of the items under it.

example: xmd 05-checklists.x.md 'checkboxes | map(.{title, mark, done})'

- title: Text: its text past the checkbox, up to its first attribute or trailing `:name`, trimmed
- name: Text or Null: its trailing `:name`
- name_range: the LSP range of its name, or null
- mark: `open`, `in_progress` or `done`: what its checkbox says
- done: Boolean: checked, or every subitem done when it has some
- parent: Number or Null: the line of the item it nests under, the nearest one indented less, until a heading
- children: List of Number: the lines of the items nested under it
- indent: Number: its indentation in bytes
- checkbox: the LSP range of its `[ ]`
- range: the LSP range of the line's text, the blanks around it aside

## attributed

Every line that writes an attribute a module declares, tasks included, with each attribute evaluated for the request's day.

example: xmd 09-appointments.x.md 'attributed | map(.{title, task})'

- title: Text: the line's text from where it starts (past a list marker and any checkbox) up to its first attribute, trimmed
- block: `item`, `prose` or `row`
- task: Boolean: whether the line is a task
- range: the LSP range of the line's text, the blanks around it aside
- attributes: Record: each attribute the line writes, by key, as `{text, value, date, error, range, value_range}`: `text` as written, `value` evaluated (null when it fails), `date` its calendar day when it is a date or time, `error` why it failed or null, and the LSP ranges of the whole `@key(value)` and of the value

## entries

Everything with a place on a timeline: leaf tasks, appointments and itinerary stops, together.

example: xmd 09-appointments.x.md 'entries | map(.{kind, title, at})'

## events

Every appointment: a line with `@at` that is not a task.

example: xmd 09-appointments.x.md 'events | map(.{title, at})'

## stops

Every stop of an itinerary: a timed line under a day heading.

example: xmd 17-itinerary.x.md 'stops | map(.{title, at}) | slice(0, 3)'

## days

Every day heading of an itinerary, `## Friday, November 20`.

example: xmd 17-itinerary.x.md 'days | map(.title)'

- value: Record: the day as read: its `day`, `month`, `year` and `weekday`, its `places`, its `stops`, and the `forecast` for it once one is fetched

## timers

Every timer: a `countdown` or `stopwatch` definition, and every name that reads one.

example: xmd 10-timers.x.md 'timers | map(.{name, state: .value.state})'

- name: Text: the timer's name
- value: Record: its state: `state` (`idle`, `running` or `paused`), `running`, `elapsed`, `duration`, `remaining` for a countdown, and when it `started`
- definition: Boolean: whether this is where the timer is defined, rather than read
- inlay: Boolean: whether your editor draws the timer's state here
- origin: Record: the `document` and `name` of the definition it belongs to

# links

Links in notes, and what is known about what they point at.

## links

Every link in a note: urls, `geo:` places and paths to files.

example: xmd 18-links-and-files.x.md 'links | map(.url)'

- url: Text: the target as written

## resources

Every target a link points at, with what a refresh last fetched about it.

example: xmd 18-links-and-files.x.md 'resources | map(.{target, metadata})'

- target: Text: the url, place or path
- metadata: Record or Null: what a refresh last fetched about it, like a pull request's state; null before one

# notes and their parts

The notes themselves, their headings, and what modules recognize and report in them.

## notes

Every note in the workspace.

example: xmd --workspace 'notes | map(.title) | slice(0, 3)'

- title: Text: its file name
- text: Text: the whole note as written

## sections

Every heading, with the lines under it.

example: xmd 05-checklists.x.md 'sections | map(.{title, level, line})'

- title: Text: the heading's text past its `#`s
- level: Number: 1 for `#`, 2 for `##`, and so on

## recognized

Every match of a recognizer a module declares. A feature module that names it in `inputs` reads only its own recognizers' matches.

example: xmd 17-itinerary.x.md 'recognized | map(.{recognizer, text}) | slice(0, 3)'

- recognizer: Text: the recognizer's `name`
- module: Text: the id of the module that declared it
- title: Text: the matched text
- text: Text: the matched text
- range: the match's LSP range
- anchor: the LSP position just past the match, where an inlay goes
- groups: Record: each named group that took part, as `{text, range}`, with `term` (Text or Null) when the recognizer declares terms for it
- parent: Number or Null: the line of the match it is `under`

## diagnostics

Every problem in a note, as your editor underlines it.

example: printf 'total := price * 2\n' | xmd 'diagnostics | map(.{severity, message})'

- code: Text: what kind of problem it is, like `name`, `property` or `module`
- message: Text: what is wrong
- severity: `error`, `warning`, `information` or `hint`

# inspecting a note

How xmd reads a note, for tools and for debugging. `xmd ast` and `xmd graph` print these for one note.

## ast

Every node of a note's syntax tree, from the document down to each literal.

example: xmd 02-calculations.x.md 'ast | filter(fn(n) => n.kind == "definition") | map(.{name, text})'

- kind: Text: the node's kind: `document`, `section`, `line`, `definition`, `binary`, `call`, `literal` and more
- id: Text: the node's id, unique in the workspace
- parent: Text or Null: the id of the node it is in
- children: List of Text: the ids of the nodes in it
- role: Text: what it is to its parent: `expression`, `left`, `right`, `argument` and more
- text: Text: the source it covers
- title: Text: a section's heading, or a task's or line's text
- name: Text: the name it defines or reads
- operator: Text: a binary or unary operator
- parameters: List of Text: a function's parameters
- value: a literal's or an attribute's value
- level: Number: a section's heading level
- state: Text: a checkbox's mark: `open`, `in_progress` or `done`
- checked: Boolean: whether a checkbox is ticked
- target: Text: a link's target
- module: Text: the module whose recognizer matched
- groups: Record: a recognized match's named groups, as text
- column: Number: a cell's column, from 0
- index: Number: a row's or a column's position, from 0
- domain: Text: a decision column's domain, `Choice` or `Count`
- type: Text: a column's type

## graph

How a note's values depend on each other: one record per note, with its nodes and the edges between them.

example: xmd 02-calculations.x.md 'graph.nodes | map(.{kind, name})'

- nodes: List of `{id, kind, name, source, external}`: each definition and section, and each name it reads from another note (`external`)
- edges: List of `{from, to, reads}`: `from` reads `to`, at each place in `reads`
