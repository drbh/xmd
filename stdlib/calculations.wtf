// Inline calculations use the same evaluated records as `wtf query calculations`.
module := {
  api: 1,
  id: "calculations",
  kind: "feature",
  inputs: {calculations: ["anchor", "errors", "bracketed", "display", "expression"]}
}

// Show successful inline calculations at their source anchors.
collect := fn(ctx) => (
  map(
    filter(ctx.document.calculations, fn(c) => length(c.errors) == 0),
    fn(c) => {
      at: c.anchor,
      label: if(c.bracketed, c.display, "= " + c.display),
      tooltip: "`" + trim(c.expression) + "` = " + c.display
    }
  )
)
