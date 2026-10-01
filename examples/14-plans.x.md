# Plans

Names no note defines, like bagels, are what the plan decides.

400:flour_stock

bakery := maximize($3 * bagels + $1.25 * doughnuts)
| constraint   | expression                                   |
| ------------ | -------------------------------------------- |
| flour        | 12 * bagels + 6.5 * doughnuts <= flour_stock |
| milk         | bagels + 0.5 * doughnuts <= 200              |
| bagel_min    | bagels >= 12                                 |
| doughnut_min | doughnuts >= 14                              |

Bake [bakery.bagels] bagels and [bakery.doughnuts] doughnuts for [bakery].
