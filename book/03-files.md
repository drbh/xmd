# 3. notes and libraries

one language, two extensions. the name says what a file is for

- `.x.md` is a working note: prose, headings, tasks, tables. github and
  plain editors still show it as markdown
- `.xmd` is a library: only definitions, at least one a function, for
  other files to import

```xmd budget.x.md
# Budget

total := $1,200
```

```xmd fees.xmd
card := fn(amount) => amount * 3%
```

a note imports values from another note and functions from a library

```xmd
budget := import("./budget.x.md")
fees := import("./fees.xmd")

Left [budget.total - fees.card(budget.total)]
```

every tool reads both. the language server points out a file whose name
does not match what it holds. the libraries that ship with xmd are in
[the library reference](reference/libraries.md)

next: [4. modules](04-modules.md)
