Every built-in call as signature help and completion show it, what
`services/analysis/src/signature.rs` reads as its `BUILTINS` and the functions
reference is generated from. A built-in's name and tier come from `Builtin`
itself; every `Builtin` has exactly one entry here, which a test checks.

An entry's heading is the built-in's name. Then come its parameters as
`- name: kind` lines, a `returns` line (a value type's name, or prose for a
union), one line of documentation, and an `example` line with what signature
help fills in, left out when there is nothing to fill in. Prose is never
wrapped.

## import

- id: Text

returns Record

Load a module by ID, or a note by path such as import("./values.x.md"); members keep their own source.

example "format"

## solve_linear

- model: Record

returns Record

Solve a bounded linear model and return raw numeric values and status.

example model

## object

- entries: List

returns Record

Build a record from key/value pairs; duplicate keys are rejected.

example [{key: "x", value: 1}]

## parse_date

- text: Text
- format: Text

returns Date or Null

Parse a calendar date with a strftime format; invalid input returns null.

example "2026-09-18", "%F"

## parse_datetime

- text: Text
- format: Text
- offset: DateTime

returns DateTime or Null

Parse a local timestamp using a reference timestamp's offset.

example "2026-09-18 09:30", "%F %H:%M", now()

## entries

- record: Record

returns List

List key/value pairs in key order.

example {x: 1}

## number

- value: Number, Money, Ratio, Duration or Count

returns Number

Extract the numeric magnitude; durations use seconds.

example 90m

## source

- value: Any

returns Text

Format a typed scalar as a round-trippable expression.

example now()

## make_date

- year: Number
- month: Number
- day: Number

returns Date or Null

Construct a calendar date; invalid dates return null.

example 2026, 9, 18

## duration_parts

- duration: Duration

returns Record

Split integer seconds into total hours, remaining minutes and seconds without rounding.

example 90m

## merge3

- base: Text
- ours: Text
- theirs: Text

returns Record

Merge two edits of a common base line by line: {clean, text}, with conflict markers when both changed the same lines.

example base, ours, theirs

## url_encode

- text: Text

returns Text

Percent-encode text for a URL query or path segment, spaces as +.

example "New York"

## date_parts

- date: Date or DateTime

returns Record

Read year, month, day and weekday (Monday is zero).

example today()

## at_time

- date: Date
- time: Duration
- offset: DateTime

returns DateTime

Combine a date and time of day using the reference timestamp offset.

example today(), 9h, now()

## parse_time

- text: Text
- format: Text

returns Duration or Null

Parse a time of day as seconds since midnight.

example "09:30", "%H:%M"

## parse_duration

- text: Text

returns Duration or Null

Parse a written duration.

example "2h"

## pad_start

- text: Text
- width: Number
- fill: Text

returns Text

Pad text to a character width with one character.

example "3", 2, "0"

## pad_end

- text: Text
- width: Number
- fill: Text

returns Text

Pad text on the right.

example "x", 3, " "

## slice

- value: Text or List
- start: Number
- end: Number

returns Text or List

Take a half-open range; text indices count Unicode characters.

example "hello", 0, 2

## concat

- lists: List...

returns List

Concatenate lists.

example [1, 2], [3]

## trim

- text: Text

returns Text

Remove surrounding whitespace.

example " hello "

## type

- value: Any

returns Text

Get the runtime type name.

example 42

## floor

- number: Number

returns Number

Round down to an integer.

example 1.5

## round

- number: Number

returns Number

Round to the nearest integer.

example 1.5

## repeat

- text: Text
- count: Number

returns Text

Repeat text a bounded number of times.

example "█", 3

## format_date

- date: Date or DateTime
- format: Text

returns Text

Format a date or timestamp with strftime directives.

example today(), "%Y-%m-%d"

## error

- message: Text

returns Never

Return an evaluation error.

example "Missing data"

## pending

- message: Text

returns Never

Return an evaluation error that says the data is not available yet, such as a lookup nothing has fetched: hosts report it as a warning rather than a mistake in the note.

example "No cached score; run xmd refresh"

## if

- condition: Boolean
- then: Value
- else: Value

returns Value

Evaluate only the selected branch; more condition, result pairs may come before the else.

example n < 0, "negative", n == 0, "zero", "positive"

## let

- names: Record
- body: Value

returns Value

Name values for the body; each name can use the ones before it.

example {x: 2, y: x * 3}, x + y

## match

- value: Value
- case: Value
- result: Value
- otherwise: Value

returns Value

Pick the result of the first case equal to the value, else the last argument; more case, result pairs may follow the first.

example state, "open", "○", "done", "✓", "?"

## coalesce

- values: Value...

returns Value

Return the first non-null value.

example null, 1

## map

- items: List
- function: Function

returns List

Apply a pure function to every item.

example [1, 2], fn(x) => x * 2

## filter

- items: List
- predicate: Function

returns List

Keep items whose predicate returns true.

example [1, 2], fn(x) => x > 1

## sort_by

- items: List
- key: Function, desc(key), or a list of them

returns List

Stable sort by one key, or by several in order; desc(key) sorts that key descending. Nulls come last either way.

example tasks, [desc(.due), .title]

## desc

- key: Function

returns Record

Sort by a key descending, as a sort_by key.

example .due

## group_by

- items: List
- key: Function

returns List

Group by a scalar key into {key, rows} records, in first-seen order.

example [1, 2, 1], fn(x) => x

## eval

- expression: Text

returns Value

Evaluate expression text in the current document's scope.

example "price * 2"

## fold

- items: List
- initial: Value
- function: Function

returns Value

Combine items left to right with an accumulator.

example [1, 2], 0, fn(a, x) => a + x

## get

- collection: Record or List
- key: Text or Number

returns Value

Read a field or index; return null when absent.

example {name: "hello"}, "name"

## length

- value: List, Record, or Text

returns Count

Count items, fields, or Unicode characters.

example "hello"

## text

- value: Value

returns Text

Format a value as text; null remains null.

example $25

## debug

- value: Value

returns Text

Inspect any value as compact JSON text in an inlay. Records, lists, and host objects expose their fields; money, dates, durations, and ratios keep their type and units.

example {rain: 35%, pack: true}

## contains

- value: List or Text
- part: Value

returns Boolean

Test membership or a text substring.

example "hello", "ell"

## starts_with

- text: Text
- prefix: Text

returns Boolean

Test a text prefix.

example "hello", "he"

## ends_with

- text: Text
- suffix: Text

returns Boolean

Test a text suffix.

example "hello", "lo"

## split

- text: Text
- separator: Text

returns List

Split text into pieces.

example "a/b", "/"

## join

- items: List
- separator: Text

returns Text

Join a list of text.

example ["a", "b"], "/"

## lower

- text: Text

returns Text

Convert text to lowercase.

example "Hello"

## upper

- text: Text

returns Text

Convert text to uppercase.

example "Hello"

## replace

- text: Text
- from: Text
- to: Text

returns Text

Replace text occurrences.

example "hello", "h", "j"

## match_pattern

- text: Text
- pattern: Text

returns Record or Null

The first match of a regular expression, or null: `{text, start, end, groups}`, offsets counting Unicode characters as `slice` does. `groups` has every named group `(?<name>...)` as `{text, start, end}`, or null when it took no part. Matching takes time linear in the text; a pattern is limited to 4096 bytes and compiled once.

example "Ada: 42", "(?<name>\\w+): (?<n>\\d+)"

## quantize

- values: List
- levels: Number
- low: Number or Null
- high: Number or Null

returns List

Each value's level from 0 to levels - 1, in equal steps between low and high (the values' own extremes when both are null) and clipped to them; null stays null, and equal bounds put every value on the middle level. Reads magnitudes as number does, so units are the caller's to check.

example [12, 18, 9, 24], 8, null, null

## cached

- kind: Text
- key: List of one-field records
- label?: Text

returns Record or Null

The workspace's cached answer for a lookup as {value, fetched_at, source}, or null before anything has fetched it. Every key read, cached or not, is one xmd refresh and the ⟳ lookups lens fetch, through the provider for its kind; hovers name it by its label and show its age. The key's parts are in the order the cache spells them: [{from: "EUR"}, {to: "USD"}] is rate:EUR:USD.

example "rate", [{from: "EUR"}, {to: "USD"}], "rate EUR→USD"

## make_money

- amount: Number
- currency: Text

returns Money or Null

An amount of money in a currency code, or null when the code is not three uppercase letters.

example 12.5, "EUR"

## make_ratio

- fraction: Number

returns Ratio

A number as a ratio: 0.4 is 40%.

example 0.4

## tagged

- kind: Text
- fields: Record
- display: Text
- hover?: Markdown

returns the kind

A record a note sees as a kind of its own, named by the module: a capitalized name of letters, digits and underscores that no built-in kind has. It reads its fields, names the kind in type, hovers and errors, shows the display text wherever it is shown, adds the hover to a symbol's hover, and is the plain record in queries and JSON. A record whose origin field is null learns the definition whose whole expression is the call that built it: {document, name, line, text, range, function, arguments}.

example "Reading", {celsius: 21}, "21°C"

## clocked

- value: Value
- ticking: Function

returns Value

The value, which keeps depending on the clock it read only while ticking(value) is true: once ticking says no, the value no longer moves with the clock it read, so nothing refreshes it.

example state, fn(s) => s.running

## next_occurrence

- rule: Text
- anchor: Date
- after: Date

returns Record

`{date, error}`: the first date after `after` that a recurrence counted from `anchor` falls on, or null with why there is none. A recurrence is `day`, `week`, `month` or `year` (or `daily`, `weekly`, `monthly`, `yearly`), or a positive whole-day duration such as `2w`. Months and years keep the anchor's day, or the month's last day when it has none (an anchor on the 31st comes back on the 30th, then the 31st).

example "month", 2026-01-31, today()

## to_json

- value: Text, Number, Boolean, Null, List or Record

returns Text

The value as compact JSON, record keys sorted. Anything else, a date included, is an error: write it as text first.

example {title: "Rent", due: "2026-10-01"}

## end_position

- text: Text

returns Record

Where `text` ends as an editor counts: `{line, character}`, its line count and the UTF-16 length of its last line, or the next line's start when it ends in a line break. Where an edit appends to a note.

example ctx.document.text

## display_width

- text: Text

returns Count

How many columns `text` takes in a terminal or a monospace editor: most characters take one, wide ones such as CJK and most emoji take two, and combining marks and zero-width characters none. It aligns text where padding to `length` would not.

example "名前"

## sum

- items: List or Table
- expression?: row calculation

returns Number, Money, Ratio, or Duration

Add compatible quantities from a list, skipping nulls, or a row expression over each table row, keeping units.

example groceries, quantity * price

## today

returns Date

The current local calendar date. Updates at midnight.

## now

returns DateTime

The current timestamp. Sampled once per evaluation; live hints refresh every second.

## date

- value: Text, Date, or DateTime

returns Date or DateTime

Parse ISO or relative date text, or take a timestamp's calendar date in the request timezone.

example "next Friday"
