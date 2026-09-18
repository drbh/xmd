// Computed definitions share their evaluated values and hovers with queries.
module := {
  api: 1,
  id: "definitions",
  kind: "feature",
  inputs: {values: ["anchor", "computed", "type", "errors", "display", "hover"]}
}

// Typed resources, plans, and timers have their own presentation modules.
visible := fn(v) => (
  v.computed && length(v.errors) == 0 && contains(["Resource", "Plan", "Countdown", "Stopwatch"], v.type) == false
)

// Place the resolved value after the complete, possibly multiline declaration.
collect := fn(ctx) => (
  map(filter(ctx.document.values, visible), fn(v) => {
    at: v.anchor,
    label: "= " + v.display,
    tooltip: v.hover
  })
)
