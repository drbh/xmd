module := {api: 1, id: "itinerary_core", kind: "library"}
dates := fn(days, today) => error("ITINERARY DATES FAILURE")

// The rest of the stdlib contract.
time_text := fn(s) => "09:00"
label := fn(s) => "a stop"

// What the itinerary feature module calls, kept quiet so only the native
// report of the failing dates shows.
collect := fn(days, day) => []
hovers := fn(days, day) => []
diagnostics := fn(days, day) => []
format_days := fn(days) => []
