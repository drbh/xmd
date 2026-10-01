module := {api: 1, id: "timer", kind: "library", inputs: [], exports: []}
// Just enough of a timer module to build a timer that cannot say whether it ticks.
make := fn(kind, args) => clocked(
  tagged("Stopwatch", {limit: null, elapsed: 0s, started: now(), idle: false, origin: null}, "custom timer"),
  fn(t) => "sometimes"
)
