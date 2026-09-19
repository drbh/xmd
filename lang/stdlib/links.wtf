// Resource views are resolved through this workspace's host modules and cache.
module := {
  api: 1,
  id: "links",
  kind: "feature",
  inputs: {
    values: ["anchor", "presentation"],
    references: ["anchor", "presentation"],
    links: ["anchor", "presentation"]
  }
}

// Named resources show even their generic file, image, map, or URL presentation.
resolved := fn(records) => (
  filter(records, fn(r) => r.presentation != null)
)

// Prose URLs receive a badge only when a host module recognizes them.
known := fn(records) => (
  filter(resolved(records), fn(r) => r.presentation.known)
)

// Present named resources, bracket references, and recognized prose URLs together.
collect := fn(ctx) => (
  map(concat(resolved(ctx.document.values), resolved(ctx.document.references), known(ctx.document.links)), fn(r) => {
    at: r.anchor,
    label: r.presentation.label,
    tooltip: r.presentation.hover
  })
)
