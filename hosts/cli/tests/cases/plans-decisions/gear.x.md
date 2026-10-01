[gear] := table
| item   | weight | value | take? |
| ------ | ------ | ----- | ----- |
| tent   | 3      | 9     |       |
| stove  | 1      | 4     |       |
| camera | 2      | 7     |       |
| books  | 4      | 3     |       |
[pack] := maximize(sum(gear, value * take))
| constraint | expression                    |
| ---------- | ----------------------------- |
| weight     | sum(gear, weight * take) <= 6 |
[menu] := table
| dish  | cost | protein | servings# |
| ----- | ---- | ------- | --------- |
| beans | $2   | 15      |           |
| eggs  | $3   | 12      |           |
[diet] := minimize(sum(menu, cost * servings))
| constraint | expression                          |
| ---------- | ----------------------------------- |
| protein    | sum(menu, protein * servings) >= 50 |
[wrong] := sum(gear, weight * take)
