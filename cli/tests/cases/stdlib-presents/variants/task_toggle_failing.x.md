module := {api: 1, id: "task", kind: "library"}
toggle := fn(recurring, done) => error("TASK TOGGLE FAILURE")

// The rest of the stdlib contract, which answers normally.
hover := fn(t) => "task hover"
checklist := fn(done, total) => "checklist"
