// Summarize headings.
module := {
  api: 1,
  id: "headings",
  kind: "feature",
  inputs: ["sections"]
}
// Use the selected section anchors.
collect := fn(ctx) => (
  map(
    ctx.document.sections,
    fn(s) => {at: s.anchor, label: s.title}
  )
)
