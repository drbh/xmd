module := {api: 1, id: "custom", kind: "feature", inputs: []}
collect := fn(ctx) => [{line: 0, label: source(now())}, {line: 0, label: "second"}]
