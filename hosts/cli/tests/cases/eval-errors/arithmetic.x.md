# Arithmetic

[money_product] := $2 * $3
[scalar_by_money] := 2 / $4
[money_by_money] := $6 / $3
[fractional] := 1s * 1.5
[fractional_divide] := 7s / 2
[negate_text] := -"text"
[not_number] := !3
[plus_text] := +"text"
[span] := 106751991167300d
[duration_sum] := span + span
[duration_min] := 0s - span - 55296s
[duration_negated] := -duration_min
[date_far] := 2026-09-16 + 100000000d
[date_huge] := 2026-09-16 + 200000000000d
[stamp_huge] := 2026-09-16T14:00:00-04:00 + 200000000000d
[stamp_far] := 2026-09-16T14:00:00-04:00 + 100000000d
[big] := 1000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
[number_over] := big * big
[number_under] := 0 - big * big
