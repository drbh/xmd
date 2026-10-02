# why xmd exists

where xmd came from, in the order it was built. the whole walk follows
one note about a weekend trip, and each block adds one idea to the last.
every block is live: edit it and watch it update. each part links to the
example or reference page that goes deeper, so this page is also a map
of everything else

## a tool for me

xmd started as a tool for me. i take notes in the same editor i write
code in, and i write them in markdown. here is a typical one

```xmd off=highlight,results,controls
# Weekend

Renting a car for $1,234 and driving up the coast.
Leaving 2026-11-20 at 09:30, about 4h on the road.

## Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```

plain markdown so far. it reads fine anywhere, but the editor knows
nothing about it: `$1,234` is just characters

## no third-party app

i wanted more from these notes, but not from a note app. apps that do
more tend to own the file and the window, and i wanted to keep plain
files and stay free to switch editors. so whatever i built had to work
inside the editor, not replace it

so a note stays a markdown file. it is named `weekend.x.md`: the `.x` is
a sub-extension in front of `.md`, so anywhere without xmd (github, a
preview pane, another editor) the file still opens and renders as plain
markdown. the syntax is chosen to read that way too: `$1,234` and
`- [ ] Pack` look fine as text. xmd adds to the file, it never locks you in.
a second extension, `.xmd`, is for libraries of functions, which comes up
in [what else](#what-else)

## color first

the first thing i missed was syntax highlighting. code gets color for
its keywords; notes are full of real-world values like money, dates,
times and durations, and those deserve color too. same note, but now xmd
knows `$1,234` is money, `2026-11-20` is a date and `4h` is a duration.
[the types example](../examples/03-types.x.md) shows every kind of value
it recognizes

```xmd off=results,controls
# Weekend

Renting a car for $1,234 and driving up the coast.
Leaving 2026-11-20 at 09:30, about 4h on the road.

## Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```

## then the editor features

with color in place, the next gaps were the editor features code takes
for granted: hovers, autocomplete and code lenses. a hover should explain
a value, a click should tick a checkbox, and a checklist should show its
progress inline. still the same note, now with progress on the heading
and a ✓ done beside each open task. click one. more in
[the checklists example](../examples/05-checklists.x.md)

```xmd
# Weekend

Renting a car for $1,234 and driving up the coast.
Leaving 2026-11-20 at 09:30, about 4h on the road.

## Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```

## lsp first

all of that lives in a language server, below the editor, rather than in
an editor plugin. that one decision is what keeps xmd portable: any
editor that speaks lsp gets the same notes. there are setups for
[vs code, zed, neovim and helix](../clients/ide), and this page runs the
same engine compiled for the browser

that also means a note can live on any web page, with no build step and
nothing on your server. the engine loads from the hosted app into a
worker in the page

```html
<link rel="stylesheet" href="https://xmd.dholtz.com/lib/theme/style.css">
<div id="note"></div>
<script type="module">
  import { mountEditor } from "https://xmd.dholtz.com/lib/adapters/contenteditable.js";
  mountEditor(document.querySelector("#note"), { source: "rent := $900\nfood := $250\nTotal [rent + food]\n" });
</script>
```

`mount` gives a read-only view, `render` static html, and `onChange`
tells you what people typed. [xmd.dholtz.com/embed](https://xmd.dholtz.com/embed/)
is that page, live

## notes are half a spreadsheet

look at what ends up in notes: costs, dates, counts, deadlines. it is
the same material people put in spreadsheets, and it changes just as
often. once xmd recognized values and could draw over the note, the
obvious next step was letting those values react to each other

## values with names

a value can have a name, which makes it a variable: `$1,234:car` is
still the text you wrote, and also `car`. names do math, dates add and
subtract, and each result is drawn beside its line.
[the values example](../examples/01-values.x.md) and
[the dates example](../examples/04-dates.x.md) go further

```xmd
# Weekend

Renting a car for $1,234:car and driving up the coast.
Leaving 2026-11-20:departure at 09:30, about 4h on the road.

total := car + $90
each := total / 3
book_by := departure - 14d

## Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```

## never stale

put a name in brackets and its value appears in the sentence, so the
prose stays true too. change the car price or the date below and
everything that uses it follows. it is a spreadsheet with no cells, and
a note that is never stale

```xmd
# Weekend

Renting a car for $1,234:car and driving up the coast.
Leaving 2026-11-20:departure at 09:30, about 4h on the road.

total := car + $90
each := total / 3
book_by := departure - 14d

We each owe [each], and have to book by [book_by].

## Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```

## more than math

arithmetic is only the start. xmd can solve for a missing variable, here
how much each of us has to save a week for six weeks
([goal seek](../examples/16-goal-seek.x.md)), and convert units and
currencies ([unit conversions](../examples/21-unit-conversions.x.md)).
`import("units")` is one of the [libraries](reference/libraries.md) every
note can use

```xmd weekend.x.md
# Weekend

Renting a car for $1,234:car and driving up the coast.
Leaving 2026-11-20:departure at 09:30, about 4h on the road.

total := car + $90
each := total / 3
book_by := departure - 14d

We each owe [each], and have to book by [book_by].

$100:saved
weekly := solve(weekly * 6 + saved >= each)

units := import("units")
drive := units.show(310, "km", "mi")

## Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```

it can reach outside the note too: the status of a pull request beside
every github link, or a live price from `quote(NVDA)`. a browser page
cannot fetch those, but your editor and `xmd refresh` can. each source is
a [provider](../stdlib/providers) written in xmd

## a reactive overlay language

step back and xmd is a small language laid over markdown. every time the
note changes, xmd parses it, builds a dataflow graph, works out what
depends on what, evaluates it, and draws the results inline. in the note
above, `car` feeds `total`, `total` feeds `each`, and `each` feeds both
the sentence and `weekly`. `xmd graph weekend.x.md` prints that graph,
and [the functions reference](reference/functions.md) lists every
built-in the language has

## written in itself

once there is a language, it can carry the tool itself. the native rust
core is kept small: it parses, evaluates and speaks lsp. the features
live above it as `.xmd` modules. checklists, timers, plans, links and
quotes are all written in xmd, in [stdlib](../stdlib).
[writing modules](reference/writing-modules.md) shows how to write one
of your own

## fully extensible

because the features are xmd, anyone can add one the same way. want
bird names marked, or the next train beside each station name from your
local transit api? a small module reads the note, fetches what it needs
and draws it inline. this one marks birds

```xmd birds.xmd active=chapter
module := {
  api: 1,
  id: "birds",
  kind: "feature",
  inputs: ["recognized"],
  recognizes: [bird]
}

// Find bird names in list items, and paint them.
bird := {
  name: "bird",
  on: "item",
  pattern: "\\b(?<bird>(?i:heron|kestrel|wren))s?\\b",
  tokens: {bird: "category3"}
}

latin := {
  heron: "Ardea herodias",
  kestrel: "Falco sparverius",
  wren: "Troglodytes aedon"
}

// Put each bird's latin name beside it.
collect := fn(ctx) => map(ctx.document.recognized, fn(found) => {
  at: found.anchor,
  label: get(latin, lower(found.groups.bird.text))
})
```

a pattern finds the names and paints them, and `collect` puts each one's
latin name beside it. with the module on, the weekend note picks it up

```xmd spotted.x.md
# Weekend

## Spotted

- a heron at the harbor
- two kestrels over the coast road
- a wren in the hedge, i think
```

list a module in `modules.json` and every note you open gets it.
[writing modules](reference/writing-modules.md) has every kind of module
with a working example, the
[recognizer](reference/writing-modules.md#recognizer) reference has every
option, and [examples/modules](../examples/modules) has more

## ask from the terminal

the same engine runs in a terminal, and this is where xmd gets unusual.
a query is one expression in the note's own language, over everything
the note holds: its values, tasks, tables, links and errors. point it at
one note, or at every note in a folder with `--workspace`

```terminal weekend.x.md
xmd weekend.x.md 'each'                                         #=> $441.33
xmd weekend.x.md 'each * 3 == total'                            #=> true
xmd weekend.x.md 'tasks | filter(fn(t) => !t.done) | map(.title)'
xmd --workspace 'tasks | filter(fn(t) => t.due != null) | map(.{title, due})' --json
xmd render weekend.x.md --format text
xmd --workspace 'diagnostics' --fail-on-match
```

that is the weekend note with a terminal on it. type your own query at
the `$`, or change the note and run one again. the workspace query reads
every note in this book at once. `--json`
keeps the types, so a date stays a date and money keeps its currency.
`render` prints the note with its results, as your editor shows it. the
last line is a ci check: it fails if any note has an error

queries lean on two shorthands, and notes can use them too. `xs | f(a)`
is `f(xs, a)`, and `.title` is `fn(t) => t.title`. `.{title, due}` picks
fields the same way, `desc(key)` or a list of keys sorts by more than one
thing, and a pipeline can go on over lines that start with `|`. inside a
table cell, write the pipe as `\|`

```xmd
prices := [$3, $6, $9]
pricey := prices | filter(fn(p) => p > $5) | length
stops := [{city: "Lisbon", day: 2}, {city: "Porto", day: 5}]
order := stops
  | sort_by(desc(.day))
  | map(.city)
```

that makes notes scriptable. a shell script, a cron job, a ci step or an
agent can read them and get typed answers back, instead of scraping
markdown. `xmd refresh` fetches the lookups (pull requests, quotes,
rates), and `xmd run` runs a command module.
[the collections reference](reference/collections.md) lists every
collection and field a query can read, with an example of each

## what else

the walk above skips a lot. a few more, all live

timers are values, with start and pause controls, and a task can carry
one ([timers](../examples/10-timers.x.md))

```xmd
# Weekend

coffee := countdown(4m)
drive := stopwatch()

- [ ] Coffee before we leave @timer(coffee)
```

tables are values too: a cell can be a calculation, and a sum reads a
column ([calculated cells](../examples/12-calculated-cells.x.md)).
`sparkline` draws a series inline ([charts](../examples/13-charts.x.md))

```xmd
# Weekend

$0.14:per_km

legs := table
| leg          | km  |
| ------------ | --- |
| to the coast | 180 |
| coast road   | 60  |
| home         | 220 |

Fuel for [sum(legs, km)] km is about [sum(legs, km) * per_km].
Chance of rain, fri to sun: [sparkline([20%, 70%, 10%], 0%, 100%)]
```

tasks take attributes: a due date, an estimate, what they wait on, and
how often they repeat ([task attributes](../examples/06-task-attributes.x.md),
[dependencies](../examples/07-dependencies.x.md),
[recurring](../examples/08-recurring.x.md))

```xmd
# Weekend

- [ ] Book the ferry :ferry @due(2026-11-10) @estimate(20m)
- [ ] Print the tickets @after(ferry)
- [ ] Water the plants @every(week) @due(2026-11-19)
```

notes can share. a note imports another note's values, and a `.xmd`
library's functions. a `.xmd` file holds only definitions, at least one a
function, for other files to import; your editor points out a file whose
name does not match what it holds

```xmd fees.xmd
card := fn(amount) => amount * 3%
```

```xmd
weekend := import("./weekend.x.md")
fees := import("./fees.xmd")

Paying by card, each of us owes [weekend.each + fees.card(weekend.each)].
```

the [libraries](reference/libraries.md) that ship with xmd import the
same way, like `units` above

and more than fits here:

- [appointments](../examples/09-appointments.x.md) with `@at`, and
  [itineraries](../examples/17-itinerary.x.md) laid out by day and stop
- [plans](../examples/14-plans.x.md): `maximize` and `minimize` over a
  table, with [yes-or-no and count columns](../examples/15-decision-columns.x.md)
  as the decisions
- [links and files](../examples/18-links-and-files.x.md) as values,
  places included
- lookups for weather forecasts, currency rates and rss feeds, each a
  [provider](../stdlib/providers)
- a hosted app at [xmd.dholtz.com/docs](https://xmd.dholtz.com/docs/) for
  when you are away from your editor

[the examples](../examples) have one short note for each

## when not to use it

- it is a file: no sync, accounts or phone app, unless you use the hosted
  app
- a browser cannot fetch feeds, rates or github status; `xmd refresh` from
  the terminal can
- money math is decimal, not accounting-grade
- the solver does linear plans only
- notes that are mostly prose with no numbers are fine as plain markdown

## the arc

notes in plain markdown, then color, then editor features, then values
that compute, then a language you can query and extend from inside your
own notes. the file is still markdown the whole way. from here,
[the reference](README.md#reference) has every function, library,
collection and kind of module, and [the examples](../examples) have a
note for each feature. to try it in your own editor,
[install xmd](https://github.com/drbh/xmd#install)

back to [the book](README.md)
