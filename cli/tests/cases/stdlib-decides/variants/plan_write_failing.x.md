module := {api: 1, id: "plan", kind: "library"}
// A fixed solution for bakery.x.md, so the plan solves and only writing fails.
solve_model := fn(model) => {
  objective: $6,
  variables: [{name: "count", value: 3}],
  constraints: [{name: "limit", op: "<=", lhs: 3, rhs: 3, slack: 0, binding: true}],
  rows: {}
}
write_edits := fn(p, document) => error("PLAN WRITE FAILURE")

// The rest of the stdlib contract.
write_title := fn() => "Write decisions"
hover := fn(p) => "plan hover"
seek_boundary := fn(name, form, unit) => error("not in this stand-in")
seek_summary := fn(op, positive) => error("not in this stand-in")
