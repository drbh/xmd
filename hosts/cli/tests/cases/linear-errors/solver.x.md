# Solver boundary

missing_field := import("probe").solve({goal: "maximize"})
extra_field := import("probe").solve({goal: "maximize", variables: {}, objective: {constant: 0, terms: {}}, constraints: [], extra: 1})
wrong_type := import("probe").solve({goal: "maximize", variables: {x: {kind: 1}}, objective: {constant: 0, terms: {}}, constraints: []})
no_variables := import("probe").solve({goal: "maximize", variables: {}, objective: {constant: 0, terms: {}}, constraints: []})
bad_op := import("probe").solve({goal: "maximize", variables: {x: {kind: "continuous", lower: 0, upper: 1}}, objective: {constant: 0, terms: {x: 1}}, constraints: [{lhs: {constant: 0, terms: {x: 1}}, op: "<", rhs: {constant: 1, terms: {}}}]})
order_missing := import("probe").solve({goal: "maximize", variables: {x: {kind: "continuous", lower: 0, upper: 1}}, order: ["y"], objective: {constant: 0, terms: {x: 1}}, constraints: []})
