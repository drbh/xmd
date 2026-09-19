[![CI](https://github.com/drbh/jot/actions/workflows/ci.yml/badge.svg)](https://github.com/drbh/jot/actions/workflows/ci.yml)

`.wtf` is markdown with reactive values, a computation graph, and an LSP-native runtime.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/user-attachments/assets/f62c327c-ba0c-4d6a-8a94-5304bad54ee1">
  <img alt="Writing a note: values, calculations, dates, tables, tasks, checklists and timers resolve as they are typed" src="https://github.com/user-attachments/assets/4b1c9bbf-4af6-4395-a3a8-ade128acbc65" width="720">
</picture>

* plain text first
* reactive values inside notes
* relationships form a computation graph
* query resolved state, not just source text
* only recompute what changed
* LSP-first: references, values, errors, actions inline
* stay unstructured until structure is useful
* static file, computed overlay

## Basic Usage

The best way to get started is to simply install the lsp into your editor and start writing `.wtf` files.

The lsp will highlight and provide the computed values and errors inline. (example below uses `#` where the inlay hint would be shown)

`example.wtf`
```wtf
The most basic note could simply be a note with some expense

$1,234:car

$67:groceries

$0.01:peanuts

total := car + groceries + peanuts # = $1,301.01

total was [total] # $1,301.01
```

## Getting Data

You can also query a specific file for its computed state.

```bash
wtf query example.wtf 'total'
# $1,301.01
```
