module := {api: 1, id: "builds", kind: "link", hosts: ["ci.example"]}
inlay := fn(ctx) => if(ctx.cached == null, "build", ctx.cached.title)
// A relative program is found next to this module.
refresh := fn(url) => {program: "./bin/build-status", args: [url.path]}
decode := fn(url, data) => if(data.ok, {title: data.name}, error("build " + url.path + " is " + data.state))
