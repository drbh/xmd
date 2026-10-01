module := {api: 1, id: "quotes", kind: "provider", provides: ["quote"], inputs: []}

// Each symbol walks the step loop a different way.
step := fn(ctx) => (
  if(ctx.key.symbol == "SPIN", {state: 1},
  if(ctx.key.symbol == "TEXT", if(ctx.state == null, {state: 1, requests: "all of them"}, {value: {price: 1, answers: length(ctx.results)}, done: true}),
  if(ctx.key.symbol == "LOOPS", {state: 1, requests: "all of them"},
  if(ctx.key.symbol == "FILE", {state: 1, requests: [{kind: "read", path: "prices.json"}]},
  if(ctx.key.symbol == "NOURL", {state: 1, requests: [{kind: "http"}]},
  if(ctx.key.symbol == "DOWN", if(ctx.state == null, {state: 1, requests: [{kind: "http", url: "https://quotes.example/down"}]}, {value: {price: 2, reply: get(ctx.results, 0)}, done: true}),
  if(ctx.key.symbol == "FAIL", {error: "no such symbol"},
  if(ctx.key.symbol == "NULLERR", {error: null, value: {price: 3}, done: true},
  "not a record"))))))))
)
