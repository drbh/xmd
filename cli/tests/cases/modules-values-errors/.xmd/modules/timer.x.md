module := {api: 1, id: "timer", kind: "library", inputs: [], exports: []}
// Just enough of the engine contract to build a timer that cannot say whether it ticks.
create := fn(kind, args) => {limit: null, elapsed: 0s, started: null, idle: true}
time_dependent := fn(t) => "sometimes"
