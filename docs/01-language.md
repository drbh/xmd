# 1. the language

a note is markdown where numbers, dates and tasks have names. change one
and everything that uses it follows. the whole language is seven things

```xmd
$1,234:car                          a value with a name
total := car + $67                  a calculation          = $1,301
We have [total] left.               any value in a sentence  $1,301
2026-11-20:departure                dates do arithmetic
## Trip :trip                       a named heading counts its tasks  0/1 complete
- [ ] Pack @due(departure - 14d)    tasks know when they are due  due 2026-11-06
focus := countdown(25m)             timers are values        25:00 remaining
```

the right column is what your editor shows inline. the rest is functions:
completion and signature help describe each one as you type, and
[the examples](../lang/examples) are one short note per feature, each one
opens in the browser with nothing to install

two shorthands chain functions together

```xmd
pricey := prices | filter(fn(p) => p > $5) | length       xs | f(a) is f(xs, a)   = 2
order := stops | sort_by(desc(.day)) | map(.city)         .day is fn(s) => s.day
```

`.{city, day}` picks fields the same way, and `desc(key)` or a list of keys
sorts by more than one thing

a pipeline can continue on indented lines that start with `|`. inside a
table cell, write the pipe as `\|`

next: [2. ask a note a question](02-queries.md)
