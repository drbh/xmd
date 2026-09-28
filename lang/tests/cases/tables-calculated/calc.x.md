[$10]:unit
[3]:qty
[groceries] := table
| item  | price        |
| ----- | ------------ |
| apple | $3.30        |
| bulk  | [unit * qty] |
| one   | [unit]       |
[total] := sum(groceries, price)
