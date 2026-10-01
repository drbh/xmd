# Tables

groceries := table
| item  | quantity | price |
| ----- | -------- | ----- |
| apple | 2        | $3.30 |
| pear  | 4        | $4.30 |

total := sum(groceries, quantity * price)
units := sum(groceries, quantity)

Groceries cost [total] for [units] items, [total / units] each.
