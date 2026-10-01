module := {api: 1, id: "tickets", kind: "link", hosts: ["issues.example"], properties: ["title"]}
inlay := fn(ctx) => if(ctx.cached == null, "ticket " + ctx.url.path, ctx.cached.title)
hover := fn(ctx) => "Ticket " + ctx.url.raw
property := fn(ctx, name) => get(ctx.cached, name)

// Each ticket's refresh fails a different way.
refresh := fn(url) => (
  if(url.path == "/t/exits", {program: "sh", args: ["-c", "echo 'tracker is down' >&2; exit 3"]},
  if(url.path == "/t/missing", {program: "/nonexistent/tracker", args: []},
  if(url.path == "/t/garbled", {program: "sh", args: ["-c", "echo '<html>oops</html>'"]},
  {program: "sh", args: ["-c", "echo '{\"summary\": 5}'"]})))
)
decode := fn(url, data) => {title: "Ticket: " + data.summary}
