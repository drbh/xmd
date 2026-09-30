module := {api: 1, id: "stale", kind: "feature", inputs: {tasks: ["line", "done"]}}

collect := fn(ctx) => []

// Each open task offers an invocation that names a revision this module does
// not have, so preparing it fails and the line's controls are withheld.
actions := fn(ctx) => map(
  filter(ctx.document.tasks, fn(t) => !t.done),
  fn(t) => {line: t.line, title: "Finish", action: {kind: "invoke", document: ctx.document.uri, expected: ctx.document.text, module: ctx.module.id, revision: "stale", event: {line: t.line}}}
)

reduce := fn(ctx, event) => {kind: "edit", document: ctx.document.uri, expected: ctx.document.text, edits: []}
