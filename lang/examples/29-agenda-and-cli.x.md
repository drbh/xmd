# Agenda and the command line

Every note feeds wtf query --workspace 'tasks' and other document collections.

- [ ] Send the invoice @due(2026-09-18) #finance
- [ ] Renew the domain @due(2026-09-30) @every(year) #admin
- [ ] Draft the talk @scheduled(2026-09-19) @estimate(2h)
- Standup @at(2026-09-18T09:00-04:00)

<!-- wtf query --workspace 'map(filter(tasks, fn(t) => contains(t.tags, "finance")), fn(t) => t.title)'
filters and projects with the same functions as notes. Replace --workspace with
29-agenda-and-cli.x.md to read this file, or use wtf ast 29-agenda-and-cli.x.md and wtf graph 29-agenda-and-cli.x.md
for syntax and dependencies. Queries are read-only. wtf query --workspace 'filter(diagnostics, fn(d) => d.severity == "error")' --fail-on-match
reports errors; wtf query --workspace plans reads solutions; wtf refresh fills caches.
Use wtf query --workspace 'import("agenda").between(entries, today(), today())'
for today's agenda. Add --json to a query for structured output. -->
