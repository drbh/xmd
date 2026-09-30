module := {api: 1, id: "timer", kind: "library", inputs: [], exports: []}
// Just enough of the engine contract to build a timer that cannot say whether it ticks.
create := fn(kind, args) => {limit: null, elapsed: 0s, started: null, idle: true}
time_dependent := fn(t) => "sometimes"

// The rest of the stdlib contract, which this stand-in never reaches.
state := fn(t) => "idle"
display := fn(t) => "custom timer"
hover := fn(t) => "custom hover"
property := fn(t, name) => null
transition := fn(t, action, original) => error("not in this stand-in")
