module := {api: 1, id: "broken", kind: "feature", inputs: {tasks: ["line"]}}

collect := fn(ctx) => []

// The hook itself fails, so none of its controls can be shown.
actions := fn(ctx) => error("ACTIONS FAILURE")
