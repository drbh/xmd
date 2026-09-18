# Functions and structured values

The expression language used by notes, queries, and plugin modules supports pure
functions. Definitions still use `:=`; a function is a value:

```wtf
tax := 8%
with_tax := fn(price) => price + price * tax
prices := [$10, $20]
total := fold(map(prices, with_tax), $0, fn(total, price) => total + price)
```

Functions capture their lexical scope. Parameters are immutable and local to the
function. A function can return another function: `add := fn(x) => fn(y) => x + y`,
then `add(2)(3)` returns `5`. Global definitions resolve at the function's source
document, never against its caller's parameters. Money, durations, dates and the
other existing value types retain their normal arithmetic rules.

Definitions can span lines inside `(...)`, `[...]`, and `{...}`, or after
`=>` and other unfinished operators. Indent continued expressions. Closing
delimiters may align with the definition; the next declaration starts a new
expression even if the previous one is incomplete. `//` starts a comment through
the end of the line, including inside an expression. Quoted URLs remain strings.

```wtf
// Add tax only to positive prices.
with_tax := fn(price) => (
  if(
    price > $0,
    price + price * tax,
    $0
  )
)
```

Records use `{title: "Hello", state: "open"}` and property access uses `.title`.
Lists use `[1, 2, 3]` in expressions; bracketed references in prose keep their
existing meaning. Use a space after numeric list separators to distinguish them
from thousands separators. `get(record, "field")` and `get(list, index)` return
`null` when absent. Direct access to an unknown record field is an error.

`if(condition, yes, no)` evaluates only the selected branch. `coalesce(a, b, ...)`
evaluates until it finds a non-null result. `&&` and `||` remain short-circuiting.

The small library includes `map`, `filter`, `fold`, `get`, `length`, `text`,
`contains`, `starts_with`, `ends_with`, `split`, `join`, `lower`, `upper`, and
`replace`. `filter` requires a Boolean predicate; `fold` calls its function with
the accumulator and next item. Functions and structured expressions also work
inside query stages. Query lambdas capture the current record's fields.

Evaluation has a step budget and a 32-call function depth limit. Text and
collection results are bounded. Functions introduce no mutable variables or effectful calls.
Fetching data remains an explicit host action. Plugin evaluation also disables
the existing local-resource `exists` property. See [Functional plugins](plugins.md).

Presentation and transformation primitives also include:

| Function | Meaning |
| --- | --- |
| `slice(value, start, end)` | Half-open slice of a list or Unicode characters in text; indices beyond the end are clamped. |
| `concat(lists...)` | Concatenate lists. |
| `trim(text)` | Remove surrounding whitespace. |
| `type(value)` | Runtime type name. |
| `floor(number)`, `round(number)` | Round down or to the nearest integer. |
| `repeat(text, count)` | Repeat text within the evaluation size limits. |
| `format_date(date, format)` | Format a date or timestamp with strftime directives. |
| `error(message)` | Return an explicit evaluation error. |


Reusable libraries and kernel operations use these same expressions in notes,
queries and [plugins](plugins.md):

| Function | Meaning |
| --- | --- |
| `import(id)` | Exported definitions from a linked module; plugins declare their `imports`. |
| `entries(record)` / `object(pairs)` | Convert records to/from `{key, value}` pairs; duplicate keys are rejected. |
| `number(value)` | Numeric magnitude; durations use seconds. |
| `source(value)` | Round-trippable expression text instead of human display formatting. |
| `pad_start(text, width, fill)` / `pad_end(...)` | Pad by Unicode character count with one fill character. |
| `date_parts(value)` | Calendar year, month, day, and Monday-based weekday index. |
| `duration_parts(duration)` | Total hours, remaining minutes and seconds, retaining integer-second precision. |
| `make_date(year, month, day)` | Date or `null` when invalid. |
| `at_time(date, duration, reference)` | Combine date and time of day using the reference timestamp's offset. |
| `parse_time(text, format)` | Time of day as a duration, or `null`. |
| `parse_date(text, format)` | Calendar date, or `null`. |
| `parse_datetime(text, format, reference)` | Local timestamp using the reference's offset, or `null`. |
| `parse_duration(text)` | Duration, or `null`. |
| `solve_linear(model)` | Unit-free continuous/integer/binary model; returns status and raw values. See the model schema in [plugins](plugins.md). |

Examples: `import("format").clock(90m)` returns `"01:30:00"`;
`import("timer").create("countdown", [25m])` returns an idle state record.
These library records can be transformed with `map`, `filter`, and `fold`.
