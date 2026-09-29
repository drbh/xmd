module := {api: 1, id: "effects", kind: "command", inputs: []}

// Every answer of the previous step, one report line each.
_answers := fn(ctx) => {report: map(ctx.results, fn(r) => text(r)), done: true}

// Ask once, then report what came back.
_once := fn(ctx, requests) => if(ctx.state == null, {state: 1, requests: requests, done: false}, _answers(ctx))

_http := [
  {kind: "http", method: "post", url: "https://api.example.com/items", headers: {"X-Trace": "t1"}, json: {name: "Widget", tags: ["a", "b"]}},
  {kind: "http", method: "PATCH", url: "https://api.example.com/items", json: null},
  {kind: "http", url: "https://api.example.com/plain"},
  {kind: "http", method: "GET", url: "https://api.example.com/missing"},
  {kind: "http", method: "GET", url: "https://api.example.com/broken"},
  {kind: "http", method: "TRACE", url: "https://api.example.com/items"},
  {kind: "http", method: "GET", url: "ftp://example.com/file"},
  {kind: "http", method: "GET"},
  {kind: "http", method: "GET", url: "https://api.example.com/items", headers: {"Bad:Name": "x"}},
  {kind: "http", method: "GET", url: "https://api.example.com/items", headers: {"X-Count": 3}}
]

_env := [{kind: "env", name: "XMD_SET"}, {kind: "env", name: "XMD_EMPTY"}, {kind: "env", name: "XMD_UNSET"}]

_credential := fn(args) => [{kind: "credential", scope: "api.example.com", set: get(args.flags, "key")}]

// Remembers its first run in a file, asks to run again, and stops on the second.
_repeat := fn(ctx) => if(
  ctx.state == null,
  {state: 1, requests: [{kind: "read", path: "runs.txt"}], done: false},
  if(
    get(ctx.results, 0).ok,
    {report: ["second run saw " + trim(get(ctx.results, 0).text)], done: true},
    {
      requests: [{kind: "write", path: "runs.txt", text: "first"}],
      report: ["first run; again soon"],
      done: true,
      repeat_after: if(get(ctx.args.flags, "every") == null, 0.01, get(ctx.args.flags, "every"))
    }
  )
)

step := fn(ctx) => (
  if(get(ctx.args.flags, "mode") == "http", _once(ctx, _http),
  if(get(ctx.args.flags, "mode") == "env", _once(ctx, _env),
  if(get(ctx.args.flags, "mode") == "env-refused", _once(ctx, [{kind: "env", name: "HOME"}]),
  if(get(ctx.args.flags, "mode") == "credential", _once(ctx, _credential(ctx.args)),
  if(get(ctx.args.flags, "mode") == "credential-unscoped", _once(ctx, [{kind: "credential"}]),
  if(get(ctx.args.flags, "mode") == "repeat", _repeat(ctx),
  if(get(ctx.args.flags, "mode") == "unknown", _once(ctx, [{kind: "teleport"}]),
  if(get(ctx.args.flags, "mode") == "malformed", {requests: "all of them", done: false},
  if(get(ctx.args.flags, "mode") == "fails", {report: ["giving up"], error: "the command failed on purpose"},
  "not a record")))))))))
)
