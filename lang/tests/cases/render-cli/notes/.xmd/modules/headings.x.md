module := {api: 1, id: "headings", kind: "feature", inputs: {sections: ["anchor"]}}
collect := fn(ctx) => map(ctx.document.sections, fn(s) => {at: s.anchor, label: "custom " + format_date(now(), "%H:%M")})
