module := {api: 1, id: "values", kind: "command", inputs: []}

// What each mode asks to write: plain JSON first, then one value JSON has no spelling for.
_payload := fn(mode) => (
  if(mode == "numbers", {whole: 8, negative: 0 - 3, negative_whole: 0 - 4.0, fraction: 0.5, huge: 18446744073709551616, count: length([1, 2]), flag: true, nothing: null, list: [1, "a"]},
  if(mode == "date", {when: date("2026-09-01")},
  if(mode == "money", {amount: $5},
  if(mode == "ratio", {share: 50%},
  if(mode == "duration", {took: 90s},
  if(mode == "function", {later: fn(x) => x},
  null))))))
)

step := fn(ctx) => (
  if(ctx.state == null,
    {state: 1, requests: [{kind: "write", path: "out.json", json: _payload(ctx.args.flags.mode)}], done: false},
    {report: map(ctx.results, fn(r) => text(r)), done: true})
)
