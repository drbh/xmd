[€450]:hotel
[hotel_usd] := to(hotel, USD)
[cost] := rate(EUR, USD) * 2
[missing] := rate(GBP, USD)
[t] := table
| item | qty | price |
|---|---|---|
| apple | 2 | $3 |
| pear | 6 | $1 |
| fig | 4 | $2 |
[total] := sum(t, qty * price)
[flat] := table
| item | qty |
|---|---|
| one | 5 |
| two | 5 |
