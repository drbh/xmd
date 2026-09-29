# Decision columns

A take? column is a yes or no the plan decides for each row.

gear := table
| item   | weight | value | take? |
| ------ | ------ | ----- | ----- |
| tent   | 3      | 9     |       |
| stove  | 1      | 4     |       |
| camera | 2      | 7     |       |
| books  | 4      | 3     |       |

pack := maximize(sum(gear, value * take))
| constraint | expression                    |
| ---------- | ----------------------------- |
| weight     | sum(gear, weight * take) <= 6 |

Packing scores [pack].
