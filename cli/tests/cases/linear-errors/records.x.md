# Solution records

[gear] := table
| item | value | take? |
| ---- | ----- | ----- |
| tent | 9     |       |
[scalar] := maximize(p + 1)
| constraint | expression |
| ---------- | ---------- |
| c          | p <= 1     |
[unsolved] := maximize(q + 2)
| constraint | expression |
| ---------- | ---------- |
| c          | q <= 1     |
[strict] := maximize(r + 3)
| constraint | expression |
| ---------- | ---------- |
| c          | r <= 1     |
[unnamed] := maximize(w + 4)
| constraint | expression |
| ---------- | ---------- |
| c          | w <= 1     |
[packed] := maximize(sum(gear, value * take) + 5)
| constraint | expression           |
| ---------- | -------------------- |
| c          | sum(gear, take) <= 1 |
[chosen] := maximize(sum(gear, value * take) + 6)
| constraint | expression           |
| ---------- | -------------------- |
| c          | sum(gear, take) <= 1 |
