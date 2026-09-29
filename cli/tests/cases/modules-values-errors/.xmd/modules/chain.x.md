module := {api: 1, id: "chain", kind: "library", imports: ["off", "engine"]}
// Declared but disabled, declared and engine-only, and never declared.
via_off := fn(x) => import("off").double(x)
via_engine := fn(x) => import("engine").double(x)
via_undeclared := fn(x) => import("lib").double(x)
via_link := fn(x) => import("tickets")
