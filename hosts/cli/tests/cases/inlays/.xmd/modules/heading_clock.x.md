module := {api: 1, id: "heading_clock", kind: "feature", inputs: {sections: ["anchor"]}}
collect := fn(ctx) => map(ctx.document.sections, fn(s) => {at: s.anchor, label: format_date(now(), "%H:%M")})
