module := {api: 1, id: "timer", kind: "library"}
// A timer module whose timers cannot say where they stand.
state := fn(t) => error("TIMER STATE FAILURE")
make := fn(kind, args) => let(
  {t: {limit: null, elapsed: 41s, started: null, idle: false}},
  tagged("Stopwatch", {limit: null, elapsed: 41s, started: null, idle: false, state: state(t), origin: null}, "custom timer")
)
