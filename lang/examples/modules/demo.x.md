# Functional modules

Open this folder as a workspace to activate the files listed in .xmd/modules.json.

https://docs.example/getting-started

with_tax := fn(price) => price * 1.08
prices := [$10, $20]
total := fold(map(prices, with_tax), $0, fn(total, price) => total + price)

The total is [total].
