# Prelude

## Trip :trip

- [x] Book flights @estimate(2h)
- [ ] Pack @estimate(45m)
- [ ] Passport
  - [x] Photo @estimate(missing)
  - [ ] Form @estimate(20m)

[count] := total(trip)
[done] := completed(trip)
[left] := remaining(trip)
[work] := effort(trip)
[titles] := map(trip.tasks, .title)
[open] := trip.tasks | filter(fn(t) => !t.done) | map(.title)
[charts] := map([[1, 2, 3], [3, 2, 1]], sparkline)
[fixed] := sparkline([1, 5, 3], 0, 10)
greet := fn(name, greeting = "Hello", mark = if(greeting == "Hello", ".", "!")) => greeting + ", " + name + mark
[a] := greet("Ada")
[b] := greet("Ada", "Hi")
[c] := greet()
[d] := greet("Ada", "Hi", "?", 4)
