module := {api: 1, id: "demo", kind: "command", inputs: []}

// The first step asks for one of every effect.
_ask := fn(args) => (
  {
    state: {stage: "asked"},
    requests: [
      {kind: "env", name: "XMD_GREETING"},
      {kind: "list", path: "notes"},
      {kind: "read", path: "notes/a.txt"},
      {kind: "http", method: "GET", url: "https://api.example.com/items", pick: ["id"]},
      {kind: "credential", scope: "example.com", set: get(args.flags, "key")},
      {kind: "uuid"},
      {kind: "read", path: "notes/missing.txt"}
    ],
    report: ["asked for " + text(length(args.positional)) + " positional arguments"],
    done: false
  }
)

// One line describing every answer, in request order.
_summary := fn(r) => (
  "greeting=" + text(get(r, 0).value)
  + " files=" + join(get(r, 1).files, ",")
  + " a=" + trim(get(r, 2).text)
  + " status=" + text(get(r, 3).status)
  + " ids=" + join(map(get(r, 3).json, fn(item) => text(item.id)), ",")
  + " fields=" + text(length(entries(get(get(r, 3).json, 0))))
  + " key=" + text(get(r, 4).value)
  + " uuid=" + text(length(get(r, 5).value))
  + " missing=" + text(get(r, 6).ok)
)

// The second step writes the summary, moves one file and removes another.
_finish := fn(results) => (
  {
    state: null,
    requests: [
      {kind: "write", path: "out/summary.txt", text: _summary(results)},
      {kind: "move", from: "notes/a.txt", to: "archive/a.txt"},
      {kind: "remove", path: "notes/b.txt"}
    ],
    report: [_summary(results)],
    done: true
  }
)

step := fn(ctx) => (
  if(ctx.state == null, _ask(ctx.args), _finish(ctx.results))
)
