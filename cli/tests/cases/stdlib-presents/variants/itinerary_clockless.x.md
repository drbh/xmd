module := {api: 1, id: "itinerary_core", kind: "library"}
// The itinerary's `records` hook builds each stop's label without a clock,
// so a label that reads one fails the build: no stops, and one error.
label := fn(s) => s.title + " · " + text(today())

// The rest of what the itinerary module calls, which answers normally.
dates := fn(days, today) => map(days, fn(d) => make_date(2026, d.month, d.day))
time_text := fn(s) => "09:00"

// What the itinerary feature module calls, kept quiet.
collect := fn(days, day) => []
hovers := fn(days, day) => []
diagnostics := fn(days, day) => []
format_days := fn(days) => []
