// Bracket references use the same expression records as document queries.
module := {
  api: 1,
  id: "references",
  kind: "feature",
  inputs: {references: ["anchor", "type", "errors", "display", "hover"]}
}

// Resource and timer references are presented by their specialized modules.
visible := fn(r) => (
  length(r.errors) == 0 && contains(["Resource", "Countdown", "Stopwatch"], r.type) == false
)

// Show resolved values immediately after the closing bracket.
collect := fn(ctx) => (
  map(filter(ctx.document.references, visible), fn(r) => {
    at: r.anchor,
    label: r.display,
    tooltip: r.hover
  })
)
