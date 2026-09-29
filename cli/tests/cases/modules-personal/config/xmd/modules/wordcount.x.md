// Show how many words a note has, after its first line.
module := {
  api: 1,
  id: "wordcount",
  kind: "feature",
  inputs: {notes: ["anchor", "text"]}
}

// Markdown marks that are not words on their own.
_marks := ["#", "-", "*", "[", "]", ">", "|", "`"]

_words := fn(text) => (
  filter(
    split(replace(text, "\n", " "), " "),
    fn(w) => length(fold(_marks, w, fn(rest, mark) => replace(rest, mark, ""))) > 0
  )
)

collect := fn(ctx) => (
  map(ctx.document.notes, fn(n) => {
    at: n.anchor,
    label: text(length(_words(n.text))) + " words",
    tooltip: "Counted by wordcount.x.md"
  })
)
