module := {api: 1, id: "fixed_rates", kind: "provider", provides: ["rate"], inputs: []}

// A workspace's own rates: answered without asking any service.
step := fn(ctx) => (
  {value: {rate: if(ctx.key.to == "USD", 1.25, 2)}, source: "fixed rates", done: true}
)
