module := {api: 1, id: "quotes", kind: "provider", provides: ["quote"], inputs: []}

// One symbol per way a provider's answer can fail to become cached JSON.
_first := fn(ctx) => (
  if(ctx.key.symbol == "BIG", {state: 1, requests: [{kind: "http", url: "https://quotes.example/big"}]},
  if(ctx.key.symbol == "DATED", {value: {price: 1, on: date("2026-09-01")}, done: true},
  if(ctx.key.symbol == "REQUEST", {state: 1, requests: [{kind: "http", url: "https://quotes.example/tick", when: date("2026-09-01")}]},
  {state: 1, requests: [{kind: "http", url: "https://quotes.example/tick"}]})))
)

step := fn(ctx) => (
  if(ctx.state == null,
    _first(ctx),
    {value: {price: get(ctx.results, 0).json.price, currency: "EUR", seen: length(ctx.results)}, done: true})
)
