// Shared presentation functions are ordinary library exports.
module := {api: 1, id: "format", kind: "library", inputs: [], exports: ["gauge", "bar", "progress", "series", "glyph", "clock", "human", "age"]}

// Keep a value within the supplied bounds.
clamp := fn(value, low, high) => (
  if(value < low, low, if(value > high, high, value))
)

// Render a fixed-width bar for a fraction between zero and one.
gauge := fn(fraction, width) => (
  repeat("█", round(clamp(fraction, 0, 1) * width))
  + repeat("░", width - round(clamp(fraction, 0, 1) * width))
)

// Add a percentage to the standard ten-cell progress bar.
bar := fn(fraction) => (
  gauge(fraction, 10) + " " + text(round(clamp(fraction, 0, 1) * 100)) + "%"
)

// A bar for `done` of `total` items; an empty set reads as no progress.
progress := fn(done, total) => (
  bar(if(total == 0, 0, done / total))
)

// Values a chart can place: numbers, money, ratios, durations and booleans.
_chartable := fn(value) => (
  contains(["Number", "Count", "Money", "Ratio", "Duration", "Boolean"], type(value))
)

// The first value whose magnitude beats every earlier one by `better`.
_extreme := fn(values, better) => (
  fold(values, get(values, 0), fn(best, v) => if(better(number(v), number(best)), v, best))
)

// Describe a chartable series as a sparkline and its smallest and largest values.
_series := fn(values) => (
  if(
    length(values) < 2,
    null,
    "`" + sparkline(map(values, fn(v) => number(v))) + "` "
    + text(_extreme(values, fn(a, b) => a < b)) + " → "
    + text(_extreme(values, fn(a, b) => a > b))
  )
)

// A sparkline plus range such as `▁▃█` 2 → 6, or null below two chartable values.
series := fn(values) => (
  _series(filter(values, _chartable))
)

// The one symbol vocabulary every label draws from. Only glyphs that every
// monospace font covers and that have no emoji form, so every editor, the
// terminal and the browser show them at the same width. A control leads with
// its glyph, and at most one lowercase word follows when the glyph alone
// would be ambiguous: "✓ done", "○ reopen", "↻ next", "↗ open".
_glyphs := {
  done: "✓",
  off: "○",
  flag: "⚑",
  repeat: "↻",
  open: "↗",
  refresh: "⟳",
  start: "▸",
  pause: "‖",
  reset: "↺"
}

// Look up one glyph by name.
glyph := fn(name) => (
  get(_glyphs, name)
)

// Render hours, minutes, and seconds without numeric grouping.
_clock_parts := fn(p) => (
  if(p.hours == 0, "", pad_start(source(p.hours), 2, "0") + ":")
  + pad_start(source(p.minutes), 2, "0")
  + ":"
  + pad_start(source(p.seconds), 2, "0")
)

// Format a duration as mm:ss or hh:mm:ss without losing seconds.
clock := fn(duration) => (
  _clock_parts(duration_parts(duration))
)

// Omit zero components from a compact travel duration.
_human_parts := fn(p) => (
  if(
    p.hours == 0,
    source(p.minutes) + "m",
    source(p.hours) + "h" + if(p.minutes == 0, "", " " + source(p.minutes) + "m")
  )
)

// Describe a duration in hours and whole minutes.
human := fn(duration) => (
  _human_parts(duration_parts(duration))
)

// Describe cache age with progressively coarser time units.
age := fn(elapsed) => (
  if(
    elapsed < 1m,
    "just now",
    if(
      elapsed < 1h,
      source(floor(elapsed / 1m)) + "m ago",
      if(
        elapsed < 1d,
        source(floor(elapsed / 1h)) + "h ago",
        if(
          elapsed < 14d,
          source(floor(elapsed / 1d)) + "d ago",
          source(floor(elapsed / 7d)) + "w ago"
        )
      )
    )
  )
)
