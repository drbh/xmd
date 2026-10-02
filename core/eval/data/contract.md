Every stdlib function native code calls, by module: what
`core/eval/src/contract.rs` reads as its `CONTRACT` and
the end of `book/reference/writing-modules.md` is generated from.

An entry's heading says whether the module must define the function
(`required` or `optional`) and names it as `module.function`. Then come its
parameters as `- name: kind` lines, a `returns` line, its role (`presents`
followed by the stand-in shown when it fails, or `decides`), and one line of
prose. Prose is never wrapped.

## required format.series

- values: List

returns Text or Null

presents no chart

A sparkline for a list of values, or null when nothing in it can be charted.

## required format.age

- elapsed: Duration

returns Text

presents the elapsed duration

How long ago cached data was fetched, in the coarsest fitting unit.

## required format.glyph

- name: Text

returns Text

presents the glyph's name

The glyph a control title starts with.

## required today.page

- entries: List
- day: Date

returns Markdown

presents nothing; the today command fails and says why

The today page: the agenda entries laid out for one day.

## required task.checklist

- done: Count
- total: Count

returns Text

presents `done/total`

The progress words a named heading's hover shows for its tasks.

## required resource.label

- resource: resource record

returns Text

presents the resource's target

A resource's inline label when no link module recognizes it.

## required resource.hover

- resource: resource record

returns Markdown

presents the resource's target

A resource's hover; a link module that recognizes the resource adds its details after it.

## required resource.control

- resource: resource record

returns Text

presents the resource's target

The title of the control that opens a resource.

## optional prelude.lookup_display

- kind: Text
- key: List of one-field records
- value: Value

returns Text

presents the cached value

How the cached value of a lookup a module's record asked for reads, or why it cannot be read.
