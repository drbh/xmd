src := import("./values.x.md")
result := src.amount + 1
See [src.amount].
local := fn(src) => src.amount
