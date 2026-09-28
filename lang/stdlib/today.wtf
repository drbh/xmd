// The today page: the agenda module picks the entries, this lays them out.
// It is a generated view — each line links back to the note and line the
// entry came from, which is where it is edited.
module := {api: 1, id: "today", kind: "library", inputs: [], exports: []}

// Engine contract: the editor calls page by name.

// The last component of a document path.
_file := fn(path) => (
  get(split(path, "/"), length(split(path, "/")) - 1)
)

// Link one entry to its source line and name what still blocks it.
_line := fn(e) => (
  "- [" + _file(e.source.path) + ":" + source(number(e.source.line)) + "](<" + e.source.uri + ">) — "
  + e.title
  + if(length(coalesce(e.blocked_by, [])) == 0, "", " (blocked by " + join(e.blocked_by, ", ") + ")")
)

// Render the agenda entries for one day as a markdown page.
page := fn(entries, day) => (
  "# Today — " + format_date(day, "%F")
  + "\n\nGenerated view. Follow a link to edit the original note.\n\n"
  + if(length(entries) == 0, "Nothing scheduled.", join(map(entries, _line), "\n"))
  + "\n"
)
