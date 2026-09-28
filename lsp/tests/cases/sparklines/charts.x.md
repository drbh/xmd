trend := sparkline([12, 18, 9, 24])
fixed := sparkline([-10, 0, 50, 100, 120], 0, 100)
rain := sparkline([20%, null, 40%, 100%], 0%, 100%)
flat := sparkline([5, 5, 5])
one := sparkline([5])
empty := sparkline([])
gaps := sparkline([null, null])
money := sparkline([$10, $30, $20])
duration := sparkline([1h, 3h, 2h])
[trend] [sparkline([3, 2, 1])]
[debug({label: "bracket ] and \"quote\"", values: [1, 2]})] then [sparkline([2, 1])]
