// Module code reaches every non-underscore member of a library it imports.
module := {api: 1, id: "consumer", kind: "library", imports: ["lib"], exports: ["doubled"]}
lib := import("lib")

// Calls a member of lib that notes cannot name.
doubled := fn(x) => lib.b(x)
