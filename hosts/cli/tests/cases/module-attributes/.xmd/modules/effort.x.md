// Declares @left, the work a line has left, and labels each line with it.
// The host evaluates the value; this module only reads it.
module := {
  api: 1,
  id: "effort",
  kind: "feature",
  inputs: ["attributed"],
  attributes: {
    left: {
      value: "duration",
      params: ["effort: Duration"],
      applies: "effort attribute",
      doc: "The work a line has left.",
      example: "1h"
    }
  }
}

collect := fn(ctx) => map(
  filter(ctx.document.attributed, fn(a) => get(a.attributes, "left") != null),
  fn(a) => {
    line: a.line,
    label: if(a.attributes.left.error == null, text(a.attributes.left.value) + " left", "?")
  }
)
