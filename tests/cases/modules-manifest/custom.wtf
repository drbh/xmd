module := {api: 1, id: "sections", kind: "feature"}
collect := fn(ctx) => map(ctx.document.sections, fn(h) => {line: h.line, label: "section · " + h.title, tooltip: "From a functional module"})
