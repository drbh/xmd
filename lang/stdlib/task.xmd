// Task presentation the editor asks for by name. Rust gathers what a task is
// (its state, blockers, estimate, timer and subtasks); this decides the words.
module := {api: 1, id: "task", kind: "library", imports: ["format"], inputs: [], exports: []}
fmt := import("format")

// Engine contract: the editor calls toggle, hover and checklist by name.

// Label the control that completes, reopens or advances a task.
toggle := fn(recurring, done) => (
  if(
    recurring,
    fmt.glyph("repeat") + " next",
    if(done, fmt.glyph("off") + " reopen", fmt.glyph("done") + " done")
  )
)

// A progress bar and count for `done` of `total` items.
_tally := fn(done, total) => (
  "`" + fmt.progress(done, total) + "` " + source(number(done)) + "/" + source(number(total))
)

// Name incomplete prerequisites, or say why they could not be resolved.
_blocked := fn(t) => (
  if(
    t.blocked_error != null,
    "\n\n" + t.blocked_error,
    if(length(t.blocked) == 0, "", "\n\nBlocked by: " + join(t.blocked, ", "))
  )
)

// Show a countdown's progress under its state; stopwatches have no limit.
_countdown := fn(timer) => (
  if(timer == null, "", if(timer.limit == null, "", "\n\n`" + fmt.bar(timer.elapsed / timer.limit) + "`"))
)

// The task's timer and, for a countdown, how far it has run.
_timer := fn(t) => (
  if(t.timer == null, "", "\n\nTimer: " + t.timer + _countdown(t.countdown))
)

// Direct subtask completion, omitted for a task without children.
_subtasks := fn(t) => (
  if(t.children == 0, "", "\n\nSubtasks: " + _tally(t.children_done, t.children))
)

// Explain a task's state, blockers, estimate, timer and subtask progress.
hover := fn(t) => (
  "**" + t.title + "**\n\n"
  + if(t.done, "Complete", if(t.in_progress, "In progress", "Incomplete"))
  + _blocked(t)
  + if(t.estimate == null, "", "\n\nEstimate: " + t.estimate)
  + _timer(t)
  + _subtasks(t)
  + "\n\nUse code actions or the clickable labels to complete/reopen tasks or control timers."
)

// Summarize a checklist's completed tasks under its value hover.
checklist := fn(done, total) => (
  _tally(done, total) + " complete"
)
