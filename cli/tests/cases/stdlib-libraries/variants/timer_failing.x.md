module := {api: 1, id: "timer", kind: "library"}
create := fn(kind, args) => error("CUSTOM FAILURE")

// The rest of the stdlib contract, which this stand-in never reaches.
state := fn(t) => "idle"
display := fn(t) => "custom timer"
hover := fn(t) => "custom hover"
property := fn(t, name) => null
transition := fn(t, action, original) => error("not in this stand-in")
