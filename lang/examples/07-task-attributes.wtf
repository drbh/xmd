# Task attributes

Attributes are `@key(value)` at the end of a task.

- [ ] Renew passport @due(2026-10-15) @estimate(45m) @tag(errands)
- [ ] Book flights @scheduled(2026-10-01) @due(2026-10-10) #travel
- [ ] Pay the deposit :deposit @estimate(10m)
- [ ] Confirm the booking @after(deposit)
- [ ] Water the plants @every(week) @due(2026-09-20)
- Dentist @at(2026-09-25T09:30-04:00)

<!-- @due and @scheduled show relative dates in inlays; @after blocks a task;
@every advances the due date when completed; a line with @at is an appointment.
Try: wtf query --workspace 'import("agenda").between(entries, today(), today() + 6d)' on the command line. -->
