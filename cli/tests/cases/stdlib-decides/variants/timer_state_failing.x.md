module := {api: 1, id: "timer", kind: "library"}
create := fn(kind, args) => {limit: null, elapsed: 41s, started: null, idle: false}
state := fn(t) => error("TIMER STATE FAILURE")

// The rest of the stdlib contract, which answers normally.
display := fn(t) => "custom timer"
hover := fn(t) => "custom hover"
property := fn(t, name) => 42s
transition := fn(t, action, original) => "stopwatch(17s)"
