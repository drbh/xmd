module := {api: 1, id: "plan", kind: "library"}
define := fn(form) => error("ACTIVE PLAN LIBRARY")

// The rest of what the plans module calls, which this stand-in never reaches.
seek := fn(form) => error("not in this stand-in")
