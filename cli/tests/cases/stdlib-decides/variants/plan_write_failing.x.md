module := {api: 1, id: "plan", kind: "library"}
// A fixed solution for bakery.x.md, with a choice its table does not hold
// yet, so the plan solves and only writing fails.
define := fn(f) => {
  value: tagged("Plan", {objective: $6, count: 3}, "$6"),
  record: {
    goal: "maximize",
    objective: $6,
    variables: {count: 3},
    variable_order: ["count"],
    constraints: [],
    columns: [
      {
        name: "count",
        cells: [
          {
            value: 3,
            label: "limit",
            document: f.document,
            source: "count <= 3",
            line: 3,
            anchor: {line: 3, character: 18},
            range: {start: {line: 3, character: 7}, end: {line: 3, character: 19}},
            width: 10
          }
        ]
      }
    ]
  }
}
write_edits := fn(p, document) => error("PLAN WRITE FAILURE")
write_title := fn() => "Write decisions"

// The rest of what the plans module calls, which this stand-in never reaches.
seek := fn(f) => error("not in this stand-in")
