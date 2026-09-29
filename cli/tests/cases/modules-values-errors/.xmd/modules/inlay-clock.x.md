module := {api: 1, id: "inlay-clock", kind: "feature", inputs: []}
collect := fn(ctx) => [{line: 0, label: "never"}]
time_dependent := fn(ctx) => error("the clock broke")
