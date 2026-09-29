# 3. notes and libraries

one language, two extensions. the name says what a file is for

- `.x.md` is a working note: prose, headings, tasks, tables. github and
  plain editors still show it as markdown
- `.xmd` is a library: only definitions, at least one a function, for
  other files to import

```xmd
budget := import("./budget.x.md")      values from another note
fees := import("./fees.xmd")           functions from a library
Left [budget.total - fees.card(budget.total)]
```

every tool reads both. the language server points out a file whose name
does not match what it holds

next: [4. modules](04-modules.md)
