module := {api: 1, id: "itinerary_core", kind: "library"}
dates := fn(days, today) => error("ITINERARY DATES FAILURE")

// A failing `dates` fails the itinerary's `records` build, which decides
// every day's date: the note has no days or stops, and one error says why.

// The rest of what the itinerary module calls.
time_text := fn(s) => "09:00"
label := fn(s) => "a stop"

// Its other hooks, kept quiet so only the failed build shows.
collect := fn(days, day) => []
hovers := fn(days, day) => []
diagnostics := fn(days, day) => []
format_days := fn(days) => []
