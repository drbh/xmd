module := {api: 1, id: "plan", kind: "library"}

// A stand-in for what a plan is worth: the objective's constant picks an
// answer the host refuses, or reads as it is.
define := fn(f) => let(
  {c: get(f.arguments, 0).constant},
  if(c == 1, 5,
  if(c == 2, {hover: "a hover and no value"},
  if(c == 3, {value: 3, hover: 7, detail: true},
  if(c == 4, {value: {objective: 4}},
  if(c == 5, error("STAND-IN REFUSES"),
  {value: tagged("Plan", {objective: 6}, "6")})))))
)

// The rest of what the plans module calls, which this stand-in never reaches.
seek := fn(f) => error("not in this stand-in")
