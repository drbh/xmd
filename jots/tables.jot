# Groceries

[groceries] := table
| item  | quantity | price |
|-------|----------|-------|
| apple | 2        | $3.30 |
| pear  | 4        | $4.30 |

[grocery_total] := sum(groceries, quantity * price)
[grocery_units] := sum(groceries, quantity)
[average_price] := grocery_total / grocery_units

Groceries will cost [grocery_total] for [grocery_units] items.

<!-- Change a quantity or add a row. Hover the total for row contributions.
Rename a column to update its formulas. Format the document to align the table. -->

# Calculated cells

[$1.50]:bulk_price
[6]:bulk_qty

[extras] := table
| item | quantity   | cost             |
| ---- | ---------- | ---------------- |
| bulk | [bulk_qty] | [bulk_price]     |
| jam  | 1          | [bulk_price * 3] |

[extras_total] := sum(extras, quantity * cost)

<!-- A bracketed cell is a calculation, like [cash] in prose: it reads named
values from any note and shows its result as an inlay. Change bulk_qty. -->
Extras cost [extras_total].
