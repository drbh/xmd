module := {api: 1, id: "t", kind: "link", hosts: ["issues.example"], path_prefix: "/tickets/"}
inlay := fn(ctx) => "ticket"
refresh := fn(url) => {program: "/bin/echo", args: ["{\"n\": 1}"]}
decode := fn(url, data) => {title: "x", when: date("2026-09-01")}
