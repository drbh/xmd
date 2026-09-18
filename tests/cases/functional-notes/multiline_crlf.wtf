// The closure keeps its defining scope.
rate := 10%
with_tax := fn(
  cost
) => (
  // Only this branch is evaluated.
  if(
    cost > $0,
    cost + cost * rate,
    $0
  )
)
result := fold(
  map([$2, $3], with_tax),
  $0,
  fn(total, price) => total + price
)
config := {
  title: "https://example.com/)", // Delimiters here are data.
  values: [
    1,
    2
  ]
}
add := fn(x) =>
  fn(y) => x + y
