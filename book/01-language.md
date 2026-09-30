# 1. the language

a note is markdown where numbers, dates and tasks have names. change one
and everything that uses it follows. the whole language is seven things,
and this note uses all of them

```xmd trip.x.md
# Trip

The car is $1,234:car for the week.

total := car + $67

We have [total] left.

2026-11-20:departure

## Packing :trip

- [ ] Pack @due(departure - 14d)

focus := countdown(25m)
```

- `$1,234:car` is a value with a name, right where it appears in a sentence
- `total := car + $67` is a calculation
- `[total]` puts any value in a sentence
- `2026-11-20:departure` is a date, and dates do arithmetic
- `## Packing :trip` is a named heading, and it counts its tasks
- `@due(departure - 14d)` tells a task when it is due
- `countdown(25m)` is a timer, and timers are values

your editor shows each result inline, after the calculation, the heading,
the task and the timer. [snapshots/01-language.txt](snapshots/01-language.txt)
is this page's notes with every inline value, as the tests see them

the rest is functions: completion and signature help describe each one as
you type, and [the function reference](reference/functions.md) lists them
all. [the examples](../lang/examples) are one short note per feature, each
one opens in the browser with nothing to install

two shorthands chain functions together. `xs | f(a)` is `f(xs, a)`, and
`.day` is `fn(s) => s.day`. `.{city, day}` picks fields the same way,
`desc(key)` or a list of keys sorts by more than one thing, and a pipeline
can continue on indented lines that start with `|`

```xmd
prices := [$3, $6, $9]
pricey := prices | filter(fn(p) => p > $5) | length
stops := [{city: "Lisbon", day: 2}, {city: "Porto", day: 5}]
order := stops | sort_by(desc(.day)) | map(.city)
cities := stops
  | sort_by(.day)
  | map(.{city})
```

inside a table cell, write the pipe as `\|`

next: [2. ask a note a question](02-queries.md)
