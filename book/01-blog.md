# why `xmd`?

this article tells the story of `xmd`, why it exists, what it does and how it does it.

since `xmd` is a bit of a unique approach on note taking; we'll walk through the origin story of `xmd` and build up an understand from first principles.

once we understand the "why" and "what", we'll look at "how" which is what makes `xmd` special.

### a tool for me

`xmd` is a tool made specifically for how I take notes. over the years I've tried many different note taking tools/approaches and always end up coming back to a simple markdown file.

a typical note might contain some prose, numerical values and check lists; like this example weekend trip note below.

```xmd off=highlight,results,controls
# Weekend

Renting a car for $1,234 and driving up the coast.
Leaving 2026-11-20 at 09:30, about 4h on the road.

### Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```

you might ask, why markdown? why not an app that is built specifically for this purpose? 

### no moats

well, a few key reasons
- IDE agnostic
- file as source of truth
- file as the core user interface

most note taking tools tend to own the file and the window, locking you into their tooling and requiring you to use their application.

I want my files as simple as possible, and the ability to freely switch between editors - if I'm at my computer I am likely interacting with code and don't want to jump into a different (often limited) text editor. 

so `xmd` follows suit, and the notes stay in markdown. this is even apparent in the file extension. the example file above would be named `weekend.x.md` where `.x` is a sub extension in front of `.md`.

this might seem strange at first - but multi part extensions are a well established pattern, you've probably interacted with `.tar.gz` or `.min.js` before.

the benefit of this approach is that as far as all existing tools know, this is a normal markdown file. 

you can open it up in your favorite editor and get all of the expected markdown niceties, and if you have `xmd` installed, it will seamlessly "upgrade" the file to use `xmd` features.

now let's look at what those features are.

### color good

okay, so what's the problem? markdown fixes everything right?

no. I want moooor!

the first thing that is clearly missing is colors! when switching between editing a source file and my notes - I immediately miss the nice syntax highlighting. 

an editor can syntax highlight because of well defined keywords and syntax, however notes kinda have this property too. they are full of real world values like, money, dates, times and durations. 

so that's the first thing we needed to remedy.

below is the same note, but now `xmd` knows `$1,234` is money, `2026-11-20` is a date and `4h` is a duration.

```xmd off=results,controls
# Weekend

Renting a car for $1,234 and driving up the coast.
Leaving 2026-11-20 at 09:30, about 4h on the road.

## Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```

already a lot better.

### not enough

syntax highlighting is a good start - but once added it was glaringly clear that we were missing some of the other IDE goodies. 

we want info on hover, autocomplete and code lens, clicks should do things and checklist should inlay progress information. 

still the same note - just with a little lsp sprinkled on top.

```xmd
# Weekend

Renting a car for $1,234 and driving up the coast.
Leaving 2026-11-20 at 09:30, about 4h on the road.

## Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```
 
**ps, try clicking the buttons - now our notes are interactive!

now we're starting to feel something - but it doesn't end here.

### reactivity

while using our awesome colorful new notes we still found ourselves leaving our ide for a calculator or occasionally doing mental math between dates. "how many days between June 17th and Dec 27th again?", "what's the total for all of the items we have in the notes?"

these kind of questions kept popping up and in some extreme cases we'd reach for a spreadsheet. spreadsheets operate on the same types of values we're using in our notes - costs, dates, durations and counts. 

spreadsheets also have the powerful ability to express relationships between variables and dynamically/reactively update downstream numbers when a upstream value changes. they essentially let you build computational graphs with values and operations.

we want reactivity too - and we already have most of the primitives. we can identify different types of values and can inlay information.

the only thing missing is the ability to name values aka create variables.

let's extend our weekend note example with a simple syntax to initialize vars. we can either use `VALUE:name` or `name := VALUE`, and finally we can operate on those values and directly inlay the output.

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

we can do all kinda of math operations directly in our note so we never have to reach for a calculator or break our brains with duration math.

in other words; we now have spreadsheets with no cells.

**ps try changing the price of `car` and watch all of the related variables react instantly. 

### more than math

we won't get too deep into this now - but we can do much more than simple math. `xmd` has the ability to solve for missing variables, convert units and fetch external information (like the status of a github pr, or weather forecast...)

all of these operations are possible using the combination of identifying values, applying operations and emitting lsp features like inlays.

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

## Packing

- [x] Book the car
- [ ] Pack
- [ ] Charge the camera
```

### am I the problem

at this point a thoughtful reader might ask; wait... didn't you just reinvent the exact thing you were trying to avoid (locked to a specific app)

no. and to appreciate why not - let's dive into the "how" `xmd` works.

firstly, `xmd` is lower level than all other note taking apps (that I'm aware of) and moves all of the logic into a protocol that practically all ides already support - the language server protocol.

what that means in practice, is that `xmd` does not require any specific text editor - from the editor perspective `xmd` is simply another language to support (most devs have added a new language to an editor before).

and equally important - `.x.md` files always gracefully fallback to markdown so there is no "importing" or "exporting" of files. just adding the `.x` enables the superpowers.

we are not the problem!

### `xmd` in `xmd`

its clear that we've given our markdown some magical powers - but what if you are not me, and you want your notes to have different superpowers?

first let's take a step back and look at the architecture of `xmd` and we'll get back to customization in a second.

I like to call `xmd` an "overlay language", simply because in order to support the features above we needed to define simple language constructs. we needed variables, operations and a interpreter that executes the graph. 

its an overlay since it's "overlaying" these concepts into markdown. this is starkly different that most languages that require the full file to be either source code. 

this language lives inside of our lsp, so it can access the inputs (the note text) and write output (colors, inlays, autocomplete and etc) using the lsp primitives that ides support.

so if `xmd` is a language, and technically it reads text and interacts with lsp apis - is it possible to write all of the transformation logic of `xmd` in its own language?

yes! it is.

underneath the hood - majority of `xmd`'s features are written in `xmd` - we limit the native code to only bridge the language with ide's apis.

`xmd`'s core features are in it's `stdlib` which can be seen here https://github.com/drbh/xmd/tree/main/stdlib

your first thought may be "hmmmm interesting choice..."

but your missing the point. since `xmd` is in `xmd` it makes the tool extremely malleable and hackable. 

since it's written in itself, you can write your own extensions that make use of the same primitives as `xmd`'s core features. 

let's look at an example to make this more concrete.

### latin bird names

let's say you keep a lot of notes with mentions of birds, and you don't always remember the latin names but it would be nice to inlay them automatically next to any common names.

no problem, let's write a tiny extension to add this. in `xmd` we call these modules and one for bird names might look like

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

this is just a toy example with 3 birds, but you can get much more complex and even call out to 3rd party apis (this is similar to how we inline a github PR status). 

to make use of this module, we just have to place it in `~/.config/xmd/modules/` and can be toggled on and off in `~/.config/xmd/modules.json`

now let's see what it looks like with this module applied

```xmd spotted.x.md
# Weekend

## Spotted

- a heron at the harbor
- two kestrel over the coast road
- a wren in the hedge, i think
```

`xmd` is extensible and easy to customize, if you are interested in more details on module writing check out https://xmd.dholtz.com/book/reference/writing-modules

### machines like structure

we've covered why, what and how, and should have a solid understanding of `xmd` from the perspective of a note taker and a module writer.

but what if you're a machine? we've got you covered too. 

one of the annoying issues with pure markdown notes is they are just big blobs of text. so if you want to query something from the file you either have to read the whole thing - or use some kind of regex/parsing tool to try to parse the part of the file you are interested in.

since we have elevated our notes to have structure via `xmd` we can use this outside of the process of note writing.

`xmd` ships as a cli tool, and so far we've been exploring what the `xmd lsp` command enables, but the command line tool does much more. 

we can directly query a `.x.md` file using the same `xmd` language we use to write notes. this lets us query any data from the file!

below show what a query and output look like from the command line for our example note.

```terminal weekend.x.md
xmd weekend.x.md 'each'                                         #=> $441.33
xmd weekend.x.md 'each * 3 == total'                            #=> true
xmd weekend.x.md 'tasks | filter(fn(t) => !t.done) | map(.title)'
```

this essentially make our notes double as scripts, they can be programmatically queried, checked in ci and generally useful outside of an editor.

### getting started

we've now covered all of the core concepts that make `xmd`, thank you for reading this far! now you can make a serious assessment on if `xmd` is/could be useful to you.

if you are still hesitant to add it to your ide you can take it for a spin in the document web application https://xmd.dholtz.com/docs/#/ which is powered by a wasm build of the lsp so its almost 1:1 with the experience in your editor.

### advance stuff

there are many features that have not been covered in this article, including nice support for tables, inline barcharts, timers, check list dependencies, custom functions and importable libraries. you can learn about all of these features in the documentation and/or dive into the examples via the web app https://github.com/drbh/xmd/tree/main/examples#examples
​