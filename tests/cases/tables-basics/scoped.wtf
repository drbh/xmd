[groceries] := table
| item | quantity | price |
| --- | --- | --- |
| apple | 2 | $3.30 |
| pear | 4 | $4.30 |

[999]:price
[999]:quantity
[other] := table
| quantity | price |
| --- | --- |
| 3 | $1.00 |
[nested] := sum(groceries, quantity * sum(other, price))
[total] := sum(groceries, quantity * price)
[dates] := table
| today | tomorrow |
| --- | --- |
| 1 | 2 |
[future] := sum(dates, today + tomorrow)
