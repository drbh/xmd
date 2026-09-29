# Checklists

A heading with :name is a checklist; tasks beneath it count.

## Launch :launch

- [x] Draft the announcement
- [x] Collect screenshots
- [ ] Record the demo :demo
  - [x] Write the script
  - [ ] Capture the recording
- [-] Update the changelog @estimate(20m)

done := completed(launch)
left := remaining(launch)
work := effort(launch)

[done] of [total(launch)] steps are done, [left] remain, about [work] of work.
Named tasks are booleans: the demo is [demo].

<!-- Try: tick a box. [-] marks a task as started: it stays open, and the
heading inlay counts it as in progress. A parent with some subtasks done is
in progress too. -->
