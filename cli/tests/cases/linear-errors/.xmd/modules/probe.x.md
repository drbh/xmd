module := {api: 1, id: "probe", kind: "library", exports: ["solve"]}

// The solver boundary, which only module code may call.
solve := fn(model) => solve_linear(model)
