# Linear plans

A plan is a calculation that chooses values. Names no note defines become
decision variables; everything else is a constant read from your notes, so the
plan re-solves as you edit the numbers around it.

## Bakery

[400]:flour_stock
[200]:milk_stock

[bakery] := maximize($3 * bagels + $1.25 * doughnuts)
| constraint   | expression                                   |
| ------------ | -------------------------------------------- |
| flour        | 12 * bagels + 6.5 * doughnuts <= flour_stock |
| milk         | bagels + 0.5 * doughnuts <= milk_stock       |
| sugar        | 2 * bagels + 0.25 * doughnuts <= 200         |
| bagel_min    | bagels >= 12                                 |
| doughnut_min | doughnuts >= 14                              |

<!-- The inlay on the plan line shows the objective and every variable. Each
constraint row shows how much of its limit is used and whether it binds.
Change flour_stock to 300 and watch the plan re-solve. -->

Bake [bakery.bagels] bagels and [bakery.doughnuts] doughnuts for [bakery].
Flour to spare: [bakery.flour]. Milk to spare: [bakery.milk].

<!-- Hover bagels for its value, or hover bakery for the full solution with
usage bars. Rename bagels: the objective, constraints, and prose follow. -->

## Staffing in hours

[weekly_hours] := 40h

[coverage] := minimize(30m * calls + 1h * visits)
| constraint | expression                  |
| ---------- | --------------------------- |
| customers  | calls + 3 * visits >= 60    |
| capacity   | 30m * calls <= weekly_hours |

Cheapest coverage takes [coverage] with [coverage.visits] visits.

<!-- Units are checked: mixing money and durations in one expression is an
error, and so is multiplying two variables together. Try either. -->

<!-- On the command line: wtf plan bakery, wtf plan bakery --export > bakery.json,
and wtf plan --import bakery.json to get the WTF source back. -->

## Goal seek

One unknown, one constraint, and the answer is the boundary value. The
definition's own name is the unknown, found through any chain of calculations.

[$1,200]:saved
[9]:months_left
[saved_by_june] := monthly * months_left + saved
[monthly] := solve(saved_by_june >= $5,000)

Save [monthly] a month to reach [saved_by_june] by June.

## Decision columns

A column named `take?` is a yes/no choice per row and `servings#` a whole
number. A plan that sums over the column decides every row; the inlay on each
cell shows the choice, and the code action on the plan line writes them in.

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
| tofu  | $4   | 20      |           |

[diet] := minimize(sum(menu, cost * servings))
| constraint | expression                          |
| ---------- | ----------------------------------- |
| protein    | sum(menu, protein * servings) >= 50 |
| variety    | sum(menu, servings) <= 4            |

Packing scores [pack]; the cheapest menu costs [diet].
