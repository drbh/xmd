module := {api: 1, id: "timer", kind: "library"}

// The constructor's first argument picks what it makes, so each way a timer
// can come out wrong is one declaration away: no record at all, a record that
// is not tagged as a timer, and a timer that is fine.
make := fn(kind, args) => get(
  {
    ok: tagged("Stopwatch", {limit: null, elapsed: 0s, started: null, idle: true, origin: null}, "custom timer", "custom hover"),
    scalar: 5,
    untagged: {limit: null, elapsed: 0s, started: null, idle: true, origin: null}
  },
  if(length(args) == 0, "ok", get(args, 0))
)
running := fn(t) => false
inlay := fn(t) => "custom timer"
actions := fn(t) => ["start"]
transition := fn(t, action, original) => error("Transitions are frozen")
