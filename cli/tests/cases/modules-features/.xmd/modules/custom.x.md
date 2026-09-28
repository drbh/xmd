module := {api: 1, id: "calculations", kind: "feature", inputs: ["calculations"]}
collect := fn(ctx) => map(ctx.document.calculations, fn(c) => {at: c.anchor, label: "custom " + c.display, tooltip: trim(c.expression)})
