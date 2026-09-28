// Exchange rates from frankfurter.dev: keyless, daily reference rates.
// A `rate` command in .xmd/providers.json replaces this provider.
module := {api: 1, id: "frankfurter", kind: "provider", provides: ["rate"], inputs: []}

// Read the one rate asked for, or say that the service has none.
_answer := fn(key, r) => (
  if(
    !r.ok,
    {error: r.error, done: true},
    if(
      type(get(get(get(r, "json"), "rates"), key.to)) == "Number",
      {value: {rate: get(get(get(r, "json"), "rates"), key.to)}, source: "frankfurter.dev", done: true},
      {error: "No rate " + key.from + "→" + key.to + " from frankfurter.dev", done: true}
    )
  )
)

step := fn(ctx) => (
  if(
    ctx.state == null,
    {
      state: "asked",
      requests: [{kind: "http", url: "https://api.frankfurter.dev/v1/latest?base=" + ctx.key.from + "&symbols=" + ctx.key.to}],
      done: false
    },
    _answer(ctx.key, get(ctx.results, 0))
  )
)
