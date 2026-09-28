rate := 10%
add := fn(x) => fn(y) => x + y
add_tax := fn(cost) => cost + cost * rate
caller := fn(rate) => add_tax($100)
result := fold(map(filter([1, 2, 3], fn(x) => x > 1), fn(x) => x * 2), 0, fn(a, b) => a + b)
