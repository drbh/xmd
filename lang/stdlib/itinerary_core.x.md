// Calendar policy and presentation use shared date and collection primitives.
module := {api: 1, id: "itinerary_core", kind: "library", inputs: [], imports: ["format"], exports: []}
fmt := import("format")
_weekdays := ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]
_months := [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December"
]

// Use the first explicit year, falling back to the request's year.
_initial_year := fn(days, day) => (
  coalesce(
    get(map(filter(days, fn(d) => d.year != null), fn(d) => d.year), 0),
    date_parts(day).year
  )
)

// Advance an omitted year across a rollover or an old first date.
_infer_year := fn(s, d, day, candidate) => (
  coalesce(
    d.year,
    s.year
    + if(
        candidate != null
        && if(s.previous == null, candidate < day - 30d, candidate < s.previous),
        1,
        0
      )
  )
)

// Append a resolved date while retaining the last valid day.
_add_date := fn(s, d, year, date) => (
  {year: year, previous: coalesce(date, s.previous), dates: concat(s.dates, [date])}
)

// Resolve a heading's month and day in the chosen year.
_date_step := fn(s, d, day, year) => (
  _add_date(s, d, year, make_date(year, d.month, d.day))
)

// Engine contract: the itinerary evaluator calls dates, time_text, canonical
// and label by name, and the itinerary feature module calls collect, hovers,
// diagnostics and format_days. These names are the engine's, not a note's:
// `exports: []` keeps them out of import().

// Carry inferred years and valid dates through the itinerary.
dates := fn(days, day) => (
  fold(
    days,
    {year: _initial_year(days, day), previous: null, dates: []},
    fn(s, d) => _date_step(
      s,
      d,
      day,
      _infer_year(s, d, day, make_date(coalesce(d.year, s.year), d.month, d.day))
    )
  ).dates
)

// Preserve the stop's original 12-hour or 24-hour clock style.
time_text := fn(s) => (
  format_date(
    at_time(make_date(2000, 1, 1), s.time, 2000-01-01T00:00:00Z),
    if(s.twelve_hour, "%I:%M %p", "%H:%M")
  )
)

// Prefix a stop title with its recognized kind marker.
label := fn(s) => (
  if(s.kind == null, "", s.kind.marker + " ") + s.title
)

// Join the normalized time, kind marker, and title.
canonical := fn(s) => (
  time_text(s) + "  " + label(s)
)

// Return time until the next stop, or null when it is out of order.
_gap := fn(s, next) => (
  if(next == null || next.time < s.time, null, next.time - s.time)
)

// Parse an explicit deadline; date-only deadlines end at 23:59.
_explicit_deadline := fn(value, reference) => (
  coalesce(
    parse_datetime(value, "%Y-%m-%d %H:%M", reference),
    parse_datetime(value, "%Y-%m-%dT%H:%M", reference),
    if(
      parse_date(value, "%Y-%m-%d") == null,
      null,
      at_time(parse_date(value, "%Y-%m-%d"), 23h + 59m, reference)
    )
  )
)

// Subtract a valid cancellation offset from the stop's time.
_relative_deadline := fn(value, offset, date, s, reference) => (
  if(offset == null, null, {at: at_time(date, s.time, reference) - offset, relative: true})
)

// Distinguish explicit deadlines from relative cancellation rules.
_deadline_value := fn(value, date, s, reference, suffix) => (
  if(
    suffix == null,
    if(
      _explicit_deadline(value, reference) == null,
      null,
      {at: _explicit_deadline(value, reference), relative: false}
    ),
    _relative_deadline(
      value,
      parse_duration(trim(slice(value, 0, length(value) - length(suffix)))),
      date,
      s,
      reference
    )
  )
)

// Resolve the first Cancel by detail when the day has a valid date.
_cancel := fn(date, s, reference) => (
  if(
    date == null || length(filter(s.details, fn(d) => lower(d.key) == "cancel by")) == 0,
    null,
    _cancel_detail(
      date,
      s,
      reference,
      trim(get(filter(s.details, fn(d) => lower(d.key) == "cancel by"), 0).value)
    )
  )
)

// Recognize the supported relative-deadline suffixes.
_cancel_detail := fn(date, s, reference, value) => (
  _deadline_value(
    value,
    date,
    s,
    reference,
    get(filter(["before", "in advance", "ahead"], fn(suffix) => ends_with(value, suffix)), 0)
  )
)

// Describe a day's distance from the request's calendar date.
_relative := fn(delta) => (
  if(
    delta == 0d,
    "today",
    if(
      delta == 1d,
      "tomorrow",
      if(
        delta > 1d,
        "in " + source(number(delta / 1d)) + " days",
        if(delta == -1d, "yesterday", source(number(-delta / 1d)) + " days ago")
      )
    )
  )
)

// Choose the earliest or latest stop without reordering the itinerary.
_extreme := fn(stops, first) => (
  fold(
    stops,
    get(stops, 0),
    fn(best, s) => if(if(first, s.time < best.time, s.time >= best.time), s, best)
  )
)

// Summarize the day, time span, relative date, and cached forecast.
_day_hint := fn(d, date, day) => (
  {
    at: d.anchor,
    label: source(number(length(d.stops)))
    + " stops"
    + if(
        length(d.stops) == 0,
        "",
        " · " + time_text(_extreme(d.stops, true)) + " – " + time_text(_extreme(d.stops, false))
      )
    + " · "
    + _relative(date - day)
    + if(d.forecast == null, "", " · " + d.forecast.display),
    tooltip: format_date(date, "%A, %B %-d, %Y")
    + " · "
    + source(date)
    + if(
        d.forecast == null,
        "",
        "\n\nForecast for "
        + d.forecast.place
        + " · "
        + fmt.age(now() - d.forecast.fetched_at)
        + " · "
        + d.forecast.source
      )
  }
)

// Distinguish arrival-to-departure layovers from ordinary gaps.
_gap_label := fn(s, next) => (
  if(
    _gap(s, next) == null || _gap(s, next) == 0s,
    [],
    [
      if(
        get(s.kind, "marker") == "<" && get(next.kind, "marker") == ">",
        "↻ " + fmt.human(_gap(s, next)) + " layover",
        "→ " + fmt.human(_gap(s, next)) + " until " + next.title
      )
    ]
  )
)

// Show a deadline and flag it once its calendar day has passed.
_cancel_label := fn(deadline, day) => (
  if(
    deadline == null,
    [],
    [
      if(parse_date(format_date(deadline.at, "%F"), "%F") < day, "! ", "")
      + "cancel by "
      + format_date(deadline.at, "%a %b %-d, %I:%M %p")
    ]
  )
)

// Combine a stop's next gap and cancellation deadline.
_stop_hint := fn(s, next, date, day) => (
  {
    at: s.anchor,
    label: join(concat(_gap_label(s, next), _cancel_label(_cancel(date, s, now()), day)), " · "),
    tooltip: s.title + " at " + time_text(s) + " on " + format_date(date, "%A, %B %-d")
  }
)

// Pair each item with its zero-based position.
_indexed := fn(items) => (
  fold(items, [], fn(out, item) => concat(out, [{index: length(out), value: item}]))
)

// Skip invalid dates and omit empty stop hints.
_collect_day := fn(d, date, day) => (
  if(
    date == null,
    [],
    concat(
      [_day_hint(d, date, day)],
      filter(
        map(_indexed(d.stops), fn(s) => _stop_hint(s.value, get(d.stops, s.index + 1), date, day)),
        fn(h) => h.label != ""
      )
    )
  )
)

// Pair each heading with its resolved calendar date.
_collect_dates := fn(days, resolved, day) => (
  fold(
    _indexed(days),
    [],
    fn(out, d) => concat(out, _collect_day(d.value, get(resolved, d.index), day))
  )
)

// Resolve the itinerary's dates before assembling its hints.
collect := fn(days, day) => (
  _collect_dates(days, dates(days, day), day)
)

// Describe a stop's kind, date, next stop, and written details.
_stop_hover := fn(s, next, date) => (
  "**"
  + s.title
  + "**\n\n"
  + if(s.kind == null, "", s.kind.name + " · ")
  + time_text(s)
  + if(date == null, "", ", " + format_date(date, "%A, %B %-d, %Y"))
  + if(_gap(s, next) == null, "", "\n\n→ " + fmt.human(_gap(s, next)) + " until " + next.title)
  + join(map(s.details, fn(d) => "\n\n**" + d.key + ":** " + d.value), "")
)

// Attach stop descriptions to their exact source ranges.
_hovers_dates := fn(days, resolved) => (
  fold(
    _indexed(days),
    [],
    fn(out, d) => concat(
      out,
      map(
        _indexed(d.value.stops),
        fn(s) => {
          range: s.value.range,
          contents: _stop_hover(s.value, get(d.value.stops, s.index + 1), get(resolved, d.index))
        }
      )
    )
  )
)

// Resolve dates for the itinerary's stop hovers.
hovers := fn(days, day) => (
  _hovers_dates(days, dates(days, day))
)

// Propose a line replacement only when its text would change.
_edit := fn(item, text) => (
  if(item.raw == text, [], [{range: item.line_range, newText: text}])
)

// Normalize one stop and indent its details and notes.
_format_stop := fn(s) => (
  concat(
    _edit(s, canonical(s)),
    fold(s.details, [], fn(out, d) => concat(out, _edit(d, "    " + d.key + ": " + d.value))),
    fold(s.notes, [], fn(out, n) => concat(out, _edit(n, "    " + trim(n.raw))))
  )
)

// Collect formatting edits for every stop in the itinerary.
format_days := fn(days) => (
  fold(
    days,
    [],
    fn(out, d) => concat(out, fold(d.stops, [], fn(edits, s) => concat(edits, _format_stop(s))))
  )
)

// Build a diagnostic with a stable code and source range.
_issue := fn(range, message, code, severity) => (
  {range: range, message: message, code: code, severity: severity, source: "wtf"}
)

// Warn about unknown kinds and report times that go backwards.
_stop_issues := fn(stops) => (
  concat(
    map(
      filter(stops, fn(s) => s.kind == null),
      fn(s) => _issue(
        s.title_range,
        "Stop has no kind; start the title with one of > < ~ @ * + ? (depart, arrive, transit, stay, meal, visit, explore)",
        "itinerary-kind",
        2
      )
    ),
    fold(
      _indexed(stops),
      [],
      fn(out, s) => concat(
        out,
        if(
          s.index > 0 && s.value.time < get(stops, s.index - 1).time,
          [
            _issue(
              s.value.time_range,
              time_text(s.value)
              + " is earlier than the previous stop at "
              + time_text(get(stops, s.index - 1)),
              "itinerary",
              1
            )
          ],
          []
        )
      )
    )
  )
)

// Check calendar validity, written weekdays, and day ordering.
_day_issues := fn(d, date, previous) => (
  if(
    date == null,
    [
      _issue(
        d.date_range,
        get(_months, d.month - 1) + " " + source(number(d.day)) + " is not a valid date",
        "itinerary",
        1
      )
    ],
    concat(
      if(
        d.weekday != null && d.weekday != date_parts(date).weekday,
        [
          _issue(
            d.weekday_range,
            format_date(date, "%B %-d, %Y")
            + " is a "
            + format_date(date, "%A")
            + ", not a "
            + get(_weekdays, d.weekday),
            "itinerary",
            1
          )
        ],
        []
      ),
      if(
        previous != null && date < previous,
        [
          _issue(
            d.date_range,
            source(date) + " comes before the previous day, " + source(previous),
            "itinerary",
            1
          )
        ],
        []
      ),
      _stop_issues(d.stops)
    )
  )
)

// Keep the last valid date while accumulating diagnostics.
_issues_step := fn(s, d, date) => (
  {
    previous: coalesce(date, s.previous),
    issues: concat(s.issues, _day_issues(d, date, s.previous))
  }
)

// Validate headings in their original order.
_issues_dates := fn(days, resolved) => (
  fold(
    _indexed(days),
    {previous: null, issues: []},
    fn(s, d) => _issues_step(s, d.value, get(resolved, d.index))
  ).issues
)

// Resolve calendar dates before checking the itinerary.
diagnostics := fn(days, day) => (
  _issues_dates(days, dates(days, day))
)
