module := {api: 1, id: "diagnostics-date", kind: "feature", inputs: []}
diagnostics := fn(ctx) => [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 1}}, message: "dated", when: date("2026-09-01")}]
