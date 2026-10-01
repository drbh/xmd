# Calculated cells

$1.50:unit_price
6:bulk_qty

extras := table
| item | quantity   | cost             |
| ---- | ---------- | ---------------- |
| bulk | [bulk_qty] | [unit_price]     |
| jam  | 1          | [unit_price * 3] |

Extras cost [sum(extras, quantity * cost)].
