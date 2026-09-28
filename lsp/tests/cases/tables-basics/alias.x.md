[alias] := import("./groceries.x.md").groceries
[cost] := sum(alias, quantity * price)
