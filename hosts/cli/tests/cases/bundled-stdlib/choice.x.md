items := table
| item | cost | take? |
| --- | --- | --- |
| Tea | $3 | |
| Pie | $5 | |
choice := maximize(sum(items, take * cost))
| constraint | expression |
| --- | --- |
| budget | sum(items, take * cost) <= $5 |
