module := {api: 1, id: "steps", kind: "command", inputs: []}

_one := [{kind: "uuid"}]
_twice := fn(list) => concat(list, list)
// 256 requests, the most one step may make.
_most := _twice(_twice(_twice(_twice(_twice(_twice(_twice(_twice(_one))))))))

// Ask once, then report how many answers came back and whether each was ok.
_once := fn(ctx, requests) => if(
  ctx.state == null,
  {state: 1, requests: requests, done: false},
  {report: [text(length(ctx.results)) + " answers", text(length(filter(ctx.results, fn(r) => r.ok))) + " ok"], done: true}
)

_mode := fn(ctx) => get(ctx.args.flags, "mode")

step := fn(ctx) => (
  if(_mode(ctx) == "most", _once(ctx, _most),
  if(_mode(ctx) == "too-many", _once(ctx, concat(_most, _one)),
  if(_mode(ctx) == "text-requests", {requests: "all of them", done: false},
  if(_mode(ctx) == "null-requests", {report: ["nothing asked"], requests: null, error: null, done: true},
  if(_mode(ctx) == "record-report", {report: "not a list", done: true},
  if(_mode(ctx) == "stored", if(
    ctx.state == null,
    {state: 1, requests: [{kind: "credential", scope: "api.example.com"}, {kind: "credential", scope: "elsewhere"}], done: false},
    {report: map(ctx.results, fn(r) => text(r.value)), done: true}),
  if(_mode(ctx) == "repeat-zero", {report: ["once"], done: true, repeat_after: 0},
  if(_mode(ctx) == "repeat-flag", {report: ["once"], done: true, repeat_after: true},
  if(_mode(ctx) == "repeat-text", {report: ["once"], done: true, repeat_after: "later"},
  {error: "unknown mode"})))))))))
)
