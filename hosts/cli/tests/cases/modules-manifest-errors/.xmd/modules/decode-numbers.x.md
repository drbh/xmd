module := {api: 1, id: "t", kind: "link", hosts: ["issues.example"], path_prefix: "/tickets/"}
inlay := fn(ctx) => "ticket"
refresh := fn(url) => {program: "/bin/echo", args: ["{\"n\": 1}"]}
decode := fn(url, data) => {title: "numbers", whole: 8, fetched: data.n, negative: 0 - 3, negative_whole: 0 - 4.0, fraction: 0.5, count: length([1, 2])}
