module := {api: 1, id: "plan", kind: "library"}

// A solver stand-in: the objective's constant picks the malformed solution.
solve_model := fn(model) => (
  if(model.objective.constant == 1, 5,
  if(model.objective.constant == 2, {variables: [], constraints: [], rows: {}},
  if(model.objective.constant == 3, {objective: 1, variables: [], rows: {}, constraints: [{name: "c", op: "<", lhs: 1, rhs: 1, slack: 0, binding: false}]},
  if(model.objective.constant == 4, {objective: 1, variables: [{name: "x"}], constraints: [], rows: {}},
  if(model.objective.constant == 5, {objective: 1, variables: [], constraints: [], rows: {}},
  {objective: 1, variables: [], constraints: [], rows: 7})))))
)

// The rest of the stdlib contract, which this stand-in never reaches.
hover := fn(p) => error("not in this stand-in")
write_edits := fn(p, document) => error("not in this stand-in")
write_title := fn() => "Write decisions"
seek_boundary := fn(name, form, unit) => error("not in this stand-in")
seek_summary := fn(op, positive) => error("not in this stand-in")
