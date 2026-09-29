module := {api: 1, id: "probe", kind: "library", exports: ["pair"]}

// eval with two arguments falls through to the one-argument tail.
pair := fn() => eval("1", "2")
