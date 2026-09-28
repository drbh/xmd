[$10]:unit
[3]:qty
[groceries] := table
| item  | price        |
| ----- | ------------ |
| apple | $3.30        |
| bulk  | [unit * qty] |
| one   | [qty]        |
[total] := sum(groceries, price)
