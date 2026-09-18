// Computed table cells retain their exact source anchor and typed display value.
module := {
  api: 1,
  id: "table_cells",
  kind: "feature",
  inputs: {cells: ["anchor", "errors", "computed", "display", "expression"]}
}

// Annotate successfully evaluated computed cells at their exact anchors.
collect := fn(ctx) => (
  map(
    filter(ctx.document.cells, fn(c) => c.computed && length(c.errors) == 0),
    fn(c) => {at: c.anchor, label: c.display, tooltip: "`" + c.expression + "` = " + c.display}
  )
)
