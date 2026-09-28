// The module-tier primitives, reached the only way a note can reach them.
module := {api: 1, id: "lifted", kind: "library", inputs: []}

// The numerical solver behind maximize and minimize.
solve := fn(model) => solve_linear(model)

// Calendar parsing, which a note gets through a library like this one.
date := fn(text, format) => parse_date(text, format)
datetime := fn(text, format, reference) => parse_datetime(text, format, reference)
