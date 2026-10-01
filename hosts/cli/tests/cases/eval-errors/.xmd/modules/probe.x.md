module := {api: 1, id: "probe", kind: "library", exports: ["evaluate", "none"]}

// eval is a module-tier special form: these reach its own arity and type checks.
evaluate := fn(source) => eval(source)
none := fn() => eval()
