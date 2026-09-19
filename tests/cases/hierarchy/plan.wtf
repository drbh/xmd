[$3,000]:budget
[$1,410]:spent
[cash] := budget - spent
[half] := cash / 2
[t] := table
| item | qty | price |
|---|---|---|
| apple | 2 | $3 |
[total] := sum(t, qty * price)
# Plan :plan
- [ ] Buy :buy @estimate(30m)
  - [x] Pick :pick
  - [ ] Pay @after(pick)
- [ ] Ship @after(buy, cash > spent)
Prose mentions [cash] without depending on it.
