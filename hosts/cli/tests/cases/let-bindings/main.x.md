# Let

price := $19.99
x := 100

subtotal := let({each: price, count: 3}, each * count)
shadowed := let({x: 1}, x + 1)
nested := map([1, 2], fn(n) => let({sq: n * n}, let({cube: sq * n}, [n, sq, cube])))
helper := let({inc: fn(n) => n + 1, twice: fn(n) => inc(inc(n))}, twice(x))

Subtotal [subtotal], shadowed [shadowed], helper [helper].
