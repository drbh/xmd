module := {api: 1, id: "review", kind: "feature", inputs: {tasks: ["title", "line", "done"]}}

// Tasks still to do.
_open := fn(ctx) => filter(ctx.document.tasks, fn(t) => !t.done)

// The whole of one line.
_line := fn(ctx, line) => {start: {line: line, character: 0}, end: {line: line, character: length(get(ctx.document.lines, line))}}

collect := fn(ctx) => map(_open(ctx), fn(t) => {line: t.line, label: "open · " + t.title, tooltip: "row " + text(t.line)})

time_dependent := fn(ctx) => false

hovers := fn(ctx) => map(ctx.document.tasks, fn(t) => {range: _line(ctx, t.line), contents: "**" + t.title + "** as of " + text(ctx.today)})

diagnostics := fn(ctx) => map(_open(ctx), fn(t) => {range: _line(ctx, t.line), severity: 3, message: "Still open: " + t.title, code: "open-task"})

// Trailing spaces on task rows go.
format := fn(ctx) => map(
  filter(ctx.document.tasks, fn(t) => get(ctx.document.lines, t.line) != trim(get(ctx.document.lines, t.line))),
  fn(t) => {range: _line(ctx, t.line), newText: trim(get(ctx.document.lines, t.line))}
)

// An open task row offers to finish it through the reducer.
actions := fn(ctx) => map(
  filter(_open(ctx), fn(t) => t.line == ctx.row),
  fn(t) => {title: "Finish " + t.title, action: {kind: "invoke", document: ctx.document.uri, expected: ctx.document.text, module: ctx.module.id, revision: ctx.module.revision, event: {line: t.line, refresh: ctx.capabilities.refresh}}}
)

reduce := fn(ctx, event) => {kind: "edit", document: ctx.document.uri, expected: ctx.document.text, edits: [{range: {start: {line: event.line, character: 3}, end: {line: event.line, character: 4}}, newText: "x"}]}
