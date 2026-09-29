[gear] := table
| item   | weight | value | take? |
| ------ | ------ | ----- | ----- |
| tent   | 3      | 9     | yes   |
| stove  | 1      | 4     | yes   |
| camera | 2      | 7     | yes   |
| books  | 4      | 3     | no    |
[pack] := maximize(sum(gear, value * take))
| constraint | expression                    |
| ---------- | ----------------------------- |
| weight     | sum(gear, weight * take) <= 6 |
