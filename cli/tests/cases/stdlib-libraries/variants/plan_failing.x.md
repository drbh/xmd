module := {api: 1, id: "plan", kind: "library"}
solve_model := fn(model) => error("ACTIVE PLAN LIBRARY")

// The rest of the stdlib contract, which this stand-in never reaches.
hover := fn(p) => error("not in this stand-in")
write_edits := fn(p, document) => error("not in this stand-in")
write_title := fn() => "Write decisions"
seek_boundary := fn(name, form, unit) => error("not in this stand-in")
seek_summary := fn(op, positive) => error("not in this stand-in")
