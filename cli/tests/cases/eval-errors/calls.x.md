# Calls

[total_bare] := total()
[today_extra] := today(1, 2)
[if_short] := if(true, 1)
[import_none] := import()
[import_number] := import(1)
[predicate] := filter([1, 2], fn(x) => x)
[sum_three] := sum(menu, qty, qty)
[sum_scalar] := sum(3)
[sum_expression] := sum(1 + 1, 2)
[sum_text] := sum(menu, dish)
[sum_empty] := sum(empty, qty)
[menu] := table
| dish  | qty |
| ----- | --- |
| beans | 2   |
[empty] := table
| dish | qty# |
| ---- | ---- |
[evaluated] := import("probe").evaluate(1)
[evaluated_none] := import("probe").none()
[evaluated_ok] := import("probe").evaluate("1 + 2")
