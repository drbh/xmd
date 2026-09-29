module := {api: 1, id: "inlay-at-date", kind: "feature", inputs: []}
collect := fn(ctx) => [{at: {line: 0, character: 0, on: date("2026-09-01")}, label: "at"}]
