# Decision columns

A column named take? is a yes/no choice per row; servings# is a whole number.
A plan that sums over the column decides every row.

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

<!-- Each cell shows its choice; the code action on the plan line writes them
into the table. Try: raise the weight limit to 8. -->
