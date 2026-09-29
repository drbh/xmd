// Stock quotes from Yahoo Finance's chart endpoint: unofficial but keyless.
// A `quote` command in .xmd/providers.json replaces this provider.
module := {api: 1, id: "yahoo_finance", kind: "provider", provides: ["quote"], inputs: []}

// The quote's metadata in a chart reply.
_meta := fn(r) => (
  get(get(get(get(get(r, "json"), "chart"), "result"), 0), "meta")
)

// Read the price and its currency, or say that there is no quote.
_answer := fn(key, r) => (
  if(
    !r.ok,
    {error: r.error, done: true},
    if(
      type(get(_meta(r), "regularMarketPrice")) == "Number",
      {
        value: {
          price: get(_meta(r), "regularMarketPrice"),
          currency: if(type(get(_meta(r), "currency")) == "Text", get(_meta(r), "currency"), "USD"),
          change_percent: get(_meta(r), "regularMarketChangePercent")
        },
        source: "finance.yahoo.com",
        done: true
      },
      {
        error: "No quote for " + key.symbol + " from finance.yahoo.com; set a quote provider in .xmd/providers.json",
        done: true
      }
    )
  )
)

step := fn(ctx) => (
  if(
    ctx.state == null,
    {
      state: "asked",
      requests: [
        {
          kind: "http",
          url: "https://query1.finance.yahoo.com/v8/finance/chart/" + url_encode(ctx.key.symbol) + "?range=1d&interval=1d"
        }
      ],
      done: false
    },
    _answer(ctx.key, get(ctx.results, 0))
  )
)
