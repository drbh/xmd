# Names and tables

[1]:dup
[2]:dup
Use [dup].
[3]:scalar
[not_table] := sum(scalar, 1)
[t] := u
[u] := t
[aliased] := sum(t, qty)
[menu] := table
| dish  | qty | qty |
| ----- | --- | --- |
| beans | 2   | 3   |
[twice] := sum(menu, qty)
[bakery] := maximize($3 * bagels)
| constraint | expression   |
| ---------- | ------------ |
| oven       | bagels <= 10 |
Bake [bakery.bagels], not [bakery.croissants].
