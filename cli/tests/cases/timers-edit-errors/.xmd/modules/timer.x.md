module := {api: 1, id: "timer", kind: "library"}

// The constructor's first argument picks the record it returns, so each way a
// malformed record can fail is one declaration away.
create := fn(kind, args) => get(
  {
    ok: {limit: null, elapsed: 0s, started: null, idle: true},
    live: {limit: null, elapsed: 7s, started: null, idle: false},
    scalar: 5,
    limit: {limit: 5, elapsed: 0s, started: null, idle: true},
    elapsed: {limit: null, elapsed: 5, started: null, idle: true},
    started: {limit: null, elapsed: 0s, started: "soon", idle: true},
    idle: {limit: null, elapsed: 0s, started: null}
  },
  if(length(args) == 0, "ok", get(args, 0))
)
time_dependent := fn(t) => if(t.elapsed == 7s, "yes", false)
state := fn(t) => "idle"
display := fn(t) => "custom timer"
hover := fn(t) => "custom hover"
property := fn(t, name) => t.elapsed
transition := fn(t, action, original) => error("Transitions are frozen")
