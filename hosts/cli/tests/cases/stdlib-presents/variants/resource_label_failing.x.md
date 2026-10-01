module := {api: 1, id: "resource", kind: "library"}
label := fn(r) => error("RESOURCE LABEL FAILURE")

// The rest of the stdlib contract, which answers normally.
hover := fn(r) => "resource hover"
control := fn(r) => "open resource"
