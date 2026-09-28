[bakery] := maximize($3 * bagels + $1.25 * doughnuts)
| constraint | expression |
| ---------- | ---------- |
| flour | 12 * bagels + 6.5 * doughnuts <= 400 |
| bagel_min | bagels >= 12 |
| doughnut_min | doughnuts >= 14 |
Bake [bakery.bagels] bagels.
