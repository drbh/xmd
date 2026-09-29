# Value builtins

made := object([{key: "b", value: 2}, {key: "a", value: [1, 2]}])
made_empty := object([])
object_row := object([1])
object_key := object([{key: 1, value: 2}])
object_value := object([{key: "a"}])
object_duplicate := object([{key: "a", value: 1}, {key: "a", value: 2}])
object_bad := object({key: "a", value: 1})
listed := entries({b: 2, a: "x"})
entries_bad := entries([1])
as_number := number($12.50)
number_duration := number(90s)
number_bool := number(true)
number_ratio := number(25%)
number_bad := number("12")
kind_number := type(1)
kind_text := type("x")
kind_money := type($1)
kind_list := type([1])
kind_null := type(null)
floored := floor(2.7)
floored_ratio := floor(250%)
rounded := round(2.5)
rounded_negative := round(-2.5)
floor_bad := floor($2.70)
round_bad := round("2")
first := coalesce(null, null, 3, 4)
nothing := coalesce(null)
empty := coalesce()
lazy := coalesce(1, error("never evaluated"))
merged := import("probe").merge("a\nb\nc\nd\n", "A\nb\nc\nd\n", "a\nb\nc\nD\n")
conflict := import("probe").merge("a\n", "b\n", "c\n")
same := import("probe").merge("a\n", "z\n", "z\n")
merge_bad := import("probe").merge("a", "b", 3)
solver := import("probe").solve({goal: "minimize", variables: {x: {kind: "continuous", lower: 2}}, objective: {constant: 0, terms: {x: 1}}, constraints: []})
solver_note := solve_linear({})

## Chores :chores

- [x] Sweep
- [ ] Mop
- [ ] Dust

left := remaining(chores)
left_bad := remaining(3)
