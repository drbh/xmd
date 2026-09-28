// A workspace library that exports one of its three names.
module := {api: 1, id: "lib", kind: "library", exports: ["a"]}

// Public: listed in exports.
a := fn(x) => x + 1

// Private to notes: defined, not exported, but reachable from another module.
b := fn(x) => x * 2

// Private everywhere: the underscore rule still applies.
_c := fn(x) => x
