// How a file, image, place or URL presents when no link module recognizes it.
// Rust resolves the target to a URL; this decides the label, hover and control.
module := {api: 1, id: "resource", kind: "library", imports: ["format"], inputs: [], exports: []}
fmt := import("format")

// Engine contract: the editor calls label, hover and control by name.

// A geo: target opens on a map rather than as a file or page.
_map := fn(r) => (
  starts_with(r.target, "geo:")
)

// Name the kind of resource, since the inlay has no richer presentation.
label := fn(r) => (
  if(
    _map(r),
    "place · open map",
    if(r.image, "image · open preview", if(starts_with(r.target, "http"), "link", "file"))
  )
)

// Link to the resource, previewing images inline.
hover := fn(r) => (
  if(
    r.url == null,
    r.error,
    "[Open " + if(_map(r), "map", "resource") + "](<" + r.url + ">)"
    + if(r.image, "\n\n![Preview](<" + r.url + ">)", "")
  )
)

// Title the control that opens the resource.
control := fn(r) => (
  fmt.glyph("open") + " " + if(r.image, "image", if(_map(r), "map", "open"))
)
