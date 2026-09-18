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
