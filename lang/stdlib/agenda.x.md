// Agenda selection is ordinary library code over the query API's entry records.
module := {api: 1, id: "agenda", kind: "library", inputs: [], exports: ["between"]}

// Keep overdue and undated tasks, scheduled events, and unresolved appointments.
_included := fn(entry, first, last) => (
  !entry.done && if(
    entry.kind == "task",
    entry.due <= last || entry.scheduled <= last || entry.at_date <= last
      || (entry.due == null && entry.scheduled == null && entry.at_date == null),
    (entry.at_date >= first && entry.at_date <= last) || length(entry.errors) > 0
  )
)

// Stable sorts put dates first, then time, document path, and source line.
_ordered := fn(entries, first) => (
  sort_by(
    sort_by(
      sort_by(
        sort_by(entries, fn(e) => e.source.line),
        fn(e) => e.source.path
      ),
      fn(e) => coalesce(text(e.at), "")
    ),
    fn(e) => coalesce(e.due, e.at_date, e.scheduled, first)
  )
)

// Select an inclusive date window from file-scoped or workspace entry records.
between := fn(entries, first, last) => (
  _ordered(filter(entries, fn(e) => _included(e, first, last)), first)
)
