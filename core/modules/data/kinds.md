How to write each kind of module. `hosts/cli/tests/reference.rs` reads this
file, with `hooks.md` for the hooks, records and steps, into
`book/reference/writing-modules.md`, and compiles every example.

`# modules` is the page's opening: prose lines, then `- name: meaning` lines
for the fields of the `module` record, in the order the page lists them. Each
`##` entry under `# kinds` is a kind of module: its name, prose lines, an
`example:` line (a module file, from the repository root, shown whole), an
optional `run:` line (a shell command run in the example's directory, its
output shown), and a `records:` line naming the `hooks.md` records its hooks
and declarations use, in the order they are shown. `# replacing a bundled
module` is the prose above the table of the functions xmd calls on a bundled
module, from `core/eval/data/contract.md`. Prose is never wrapped.

# modules

A module is a `.xmd` file that adds to xmd. It is ordinary xmd: definitions, at least one of them a function, and a `module` record that says what kind of module it is. The host calls a module's hooks, functions with fixed names, and draws or runs what they return.

List a module's file in `modules.json` to turn it on: `~/.config/xmd/modules.json` for every note you open, or `.xmd/modules.json` at a workspace's root for that workspace. A module whose `id` is a bundled module's replaces it: everything xmd draws is a module in `stdlib` you can read and replace.

One library needs no `import`: the exports of `prelude` are names in every note and module, which is where `sparkline`, `total`, `effort`, `rate`, `to`, `quote` and `forecast` come from. A note's own names come first, so a note that defines `total` reads its own.

To try one, `cp -r examples/personal/xmd ~/.config/` puts a word count after every note's title once the language server restarts. `examples/modules` and `stdlib` have more to read.

The `module` record, `module := {api: 1, id: "words", kind: "feature"}`, says what a module is:

- api: Number: always 1
- id: Text: the module's name, letters, digits, `-`, `_` and `.`
- kind: `feature`, `link`, `library`, `command` or `provider`
- enabled?: Boolean: false turns it off without removing it. True when absent
- inputs?: the collections its hooks read, as a list, or a record of the fields to keep of each: `{notes: ["anchor", "text"]}`. Sections, tasks, values and links when absent
- imports?: List of Text: the library ids it calls `import` on
- recognizes?: List of recognizers: a feature module's patterns, below
- collections?: Record: the collections a feature module builds, below
- attributes?: Record: the attributes a feature module adds, like `@due`, below
- forms?: Record: the forms a feature module adds, like `maximize`, below
- hosts?: List of Text: the hosts whose links a link module matches, or `"*"` for any it `matches`
- path_prefix?: Text: the start of the path a link module's links have
- properties?: List of Text: the fields a query reads from a link module's links, through its `property` hook
- exports?: List of Text: the names a library gives a note
- accepts?: Record: the kinds of value each of a library's functions takes, by name, like `["Duration"]`
- provides?: List of Text: the kinds of lookup a provider answers, like `rate`, `quote` or `forecast`
- cache_version?: Number: raise it when what a module caches changes shape. 1 when absent

# kinds

## feature

A feature module annotates notes: inline labels, hovers, controls, problems, completions. It can also add to the language itself: patterns it recognizes, collections a query can name, attributes like `@due`, and forms like `maximize`. Most of what xmd draws is a feature module in `stdlib`.

example: examples/personal/xmd/modules/wordcount.xmd

records: feature context, action, recognizer, collection, built record, attribute, form, formed

## link

A link module recognizes links on some hosts and says what they are: the label after a link, its hover, and fields a query can read, refreshed from the network when it needs to be.

example: examples/modules/.xmd/modules/documentation.xmd

records: url, link context

## library

A library gives notes and other modules functions to `import`. It has no hooks: what it exports is its own names, and each one's `//` comment is its description in the libraries reference.

example: examples/modules/scores.xmd

## command

A command runs from the terminal with `xmd run`. It is one `step` function in a loop: each step asks for the effects it wants, like reading a file or calling an api, the host performs them, and the next step reads their results. Every result has `ok`, so a failed effect is the module's to handle.

example: examples/modules/notes.xmd

run: xmd run ./notes.xmd

## provider

A provider answers lookups, `cached(kind, key)`: the rates `to` converts with, `quote` prices, `forecast` weather. A refresh runs its `step` loop once per lookup, with only `http` requests.

example: stdlib/providers/frankfurter.xmd

# replacing a bundled module

A module with a bundled module's id replaces it, and has to define the functions xmd calls on that module. An optional one falls back to xmd's own when it is missing. When a function that words something fails, the note shows the stand-in under *if it fails*; when one that decides something fails, the note reports it as a problem.
