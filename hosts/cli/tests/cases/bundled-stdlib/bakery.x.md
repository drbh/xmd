[400]:flour_stock
[bakery] := maximize($3 * bagels + $1.25 * doughnuts)
| constraint | expression |
| --- | --- |
| flour | 12 * bagels + 6.5 * doughnuts <= flour_stock |
| minimum | bagels >= 12 |
| doughnuts_min | doughnuts >= 14 |
Bake [bakery.bagels] for [bakery].
