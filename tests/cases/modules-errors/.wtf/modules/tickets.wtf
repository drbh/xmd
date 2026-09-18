module := {api: 1, id: "tickets", kind: "link", hosts: ["issues.example"], properties: ["title", "points"]}
inlay := fn(ctx) => if(ctx.cached == null, "ticket " + ctx.url.path, ctx.cached.title)
hover := fn(ctx) => "Details for " + ctx.url.raw
property := fn(ctx, name) => get(ctx.cached, name)
refresh := fn(url) => {program: "/bin/echo", args: ["{\"summary\":\"Ship it\",\"points\":8}"]}
decode := fn(url, data) => {title: data.summary, points: data.points}
