module := {api: 1, id: "escape", kind: "command", inputs: []}

// Paths that leave the command's directory are refused, not performed.
step := fn(ctx) => (
  if(
    ctx.state == null,
    {state: 1, requests: [{kind: "read", path: "../demo.x.md"}, {kind: "write", path: "/tmp/x", text: "no"}], done: false},
    {report: map(ctx.results, fn(r) => text(r.ok) + ": " + r.error), done: true}
  )
)
