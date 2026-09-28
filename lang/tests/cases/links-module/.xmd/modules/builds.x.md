// A workspace link module for a build service, with cached state and refresh.
module := {
  api: 1,
  id: "builds",
  kind: "link",
  hosts: ["builds.example"],
  properties: ["state", "observed", "title"],
  cache_version: 1
}

matches := fn(url) => starts_with(url.path, "/runs/")

inlay := fn(ctx) => (
  if(
    ctx.cached == null,
    "build · refresh for status",
    "build · " + ctx.cached.state
  )
)

hover := fn(ctx) => "Build status at " + text(now())

time_dependent := fn(ctx) => true

property_names := fn(url) => if(url.path == "/runs/42", ["state", "title"], [])

property := fn(ctx, name) => (
  if(
    ctx.cached == null,
    error("Refresh build first"),
    if(
      name == "observed",
      now(),
      if(
        name == "state" || name == "title",
        get(ctx.cached, name),
        error("Unknown build property '" + name + "'")
      )
    )
  )
)

refresh := fn(url) => {
  title: "⟳ build",
  program: "fake-build",
  args: [url.path],
  env: {XMD_FIXTURE_READY: "yes"}
}

decode := fn(url, data) => {title: data.title, state: data.state}
