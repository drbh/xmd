// The book: parts, chapters, and the prose around each example. Each chapter
// names one note from lang/examples/, inlined at build time by examples.js. Keep
// the prose short: the example is the explanation.
export const parts = [
  {
    title: "Values",
    intro: "Plain text that can count. Name the numbers you already write; everything else builds on that.",
    chapters: [
      { file: "01-values.wtf", title: "Naming Values", prose: [
        "Follow a value with a colon and a name: <code>$3,000:budget</code>. Read it anywhere with <code>[budget]</code>; the current value appears right after. Brackets name text with spaces (<code>\"Oaxaca City\":city</code>); <code>\\:</code> is a plain colon.",
      ] },
      { file: "02-calculations.wtf", title: "Calculations", prose: [
        "<code>remaining := budget - spent</code> defines a calculation; its result shows inline and updates when any input changes. Hover a name to see the expression with every input filled in. <code>[remaining / budget]</code> computes in place, no name needed.",
      ] },
      { file: "03-types.wtf", title: "Types", prose: [
        "Money, numbers, ratios, dates, durations, and booleans each have a type, and arithmetic respects it: money × number is money, date + duration is a date. Money × money is an error, pointed at the operator.",
      ] },
      { file: "04-dates.wtf", title: "Dates", prose: [
        "Dates are ISO (<code>2026-11-20</code>); <code>today()</code> is always today, so <code>departure - today()</code> counts down by itself. Relative words work in attributes and in <code>date(\"next Friday\")</code>; the Freeze action pins one to a concrete date.",
      ] },
      { file: "05-currencies.wtf", title: "Currencies", prose: [
        "Money keeps its currency (<code>$12</code>, <code>700 MXN</code>); mixing two is an error until you convert with <code>to(hotel, USD)</code>. Rates come from a cache filled by <code>wtf refresh</code>, never fetched while typing. This page has no cache, so conversions below are warnings.",
      ] },
      { file: "35-unit-conversions.wtf", title: "Units", prose: [
        "<code>import(\"units\")</code> loads the bundled units library: <code>convert(100, \"km\", \"mi\")</code>, <code>format</code>, and <code>dimension</code> cover temperature, distance, mass, volume, speed, area, and data.",
      ] },
    ],
  },
  {
    title: "Tasks",
    intro: "A checkbox and a few <code>@attributes</code> are the whole task syntax.",
    chapters: [
      { file: "06-checklists.wtf", title: "Checklists", prose: [
        "Name a heading and the task list under it becomes a value: <code>completed(launch)</code>, <code>remaining(launch)</code>, <code>total(launch)</code>, <code>effort(launch)</code>. Named tasks are booleans. Tick a box below and watch the gauge move.",
      ] },
      { file: "07-task-attributes.wtf", title: "Attributes", prose: [
        "<code>@due</code> and <code>@scheduled</code> take dates, <code>@estimate</code> a duration, <code>@tag</code> and <code>#hashtags</code> label, <code>@after</code> waits, <code>@at</code> makes an appointment. The inlay summarizes what matters today.",
      ] },
      { file: "08-dependencies.wtf", title: "Dependencies", prose: [
        "<code>@after</code> blocks a task on a named task, a whole checklist, or any boolean expression. Blocked tasks say what they wait for; a cycle is an error at both ends.",
      ] },
      { file: "09-recurring.wtf", title: "Recurring", prose: [
        "<code>@every</code> repeats by day, week, month, year, or a duration like <code>2w</code>. Completing advances the due date and records the completion instead of ticking the task off.",
      ] },
      { file: "10-events.wtf", title: "Appointments", prose: [
        "A line with <code>@at</code> and a time is an appointment; give the time an offset. It is also a value: <code>now()</code> subtracts from it to count down.",
      ] },
      { file: "11-timers.wtf", title: "Timers", prose: [
        "<code>countdown(25m)</code> and <code>stopwatch()</code> tick inline and expose <code>.remaining</code> and <code>.running</code>. Start, pause, and reset are actions that rewrite the definition; ticking never edits the file.",
      ] },
    ],
  },
  {
    title: "Tables and Plans",
    intro: "Typed tables, then a solver that decides values under constraints.",
    chapters: [
      { file: "12-tables.wtf", title: "Tables", prose: [
        "A named value followed by a Markdown table. Columns are typed identifiers; <code>sum(table, quantity * price)</code> evaluates per row. Renaming a column updates formulas; bad cells are flagged in place; numeric columns get a sparkline on hover.",
      ] },
      { file: "13-calculated-cells.wtf", title: "Calculated Cells", prose: [
        "A cell in brackets is a calculation and can read any named value, so a table follows numbers defined elsewhere.",
      ] },
      { file: "14-plans.wtf", title: "Linear Plans", prose: [
        "<code>maximize</code> or <code>minimize</code> an expression over a constraint table. Undefined names become decision variables; defined ones are constants, so the plan re-solves as notes change. Each constraint shows its usage and whether it binds.",
      ] },
      { file: "15-decision-columns.wtf", title: "Decision Columns", prose: [
        "A column named <code>take?</code> is a yes/no per row and <code>servings#</code> a whole number; a plan summing over it decides every row. An action writes the choices back into the table.",
      ] },
      { file: "16-goal-seek.wtf", title: "Goal Seek", prose: [
        "<code>solve(constraint)</code> makes the definition's own name the unknown and finds the boundary value where the constraint holds, exactly for linear chains.",
      ] },
    ],
  },
  {
    title: "Outside Data",
    intro: "External values arrive through a cache you refresh on purpose; notes never phone out while you type.",
    chapters: [
      { file: "17-resources.wtf", title: "Resources and Links", prose: [
        "Links, files, places, and GitHub items are values with an Open lens. Refreshed pull requests, issues, and commits expose <code>.merged</code>, <code>.checks_passed</code>, and a status badge.",
      ] },
      { file: "18-lookups.wtf", title: "Weather and Quotes", prose: [
        "<code>forecast(place, date)</code> gives high, low, summary, and rain chance; <code>quote(NVDA)</code> is a price. Hover shows the source and age. Providers are keyless and replaceable.",
      ] },
      { file: "33-images-and-files.wtf", title: "Images and Files", prose: [
        "Relative and home-relative paths resolve from the note; images preview on hover, files open with a lens.",
      ] },
    ],
  },
  {
    title: "Trips",
    intro: "Itineraries are recognized from their shape: days, times, and details.",
    chapters: [
      { file: "19-itinerary.wtf", title: "Itineraries", prose: [
        "A weekday-and-date line starts a day; a time starts a stop, with one marker for its kind: <code>&gt;</code> depart, <code>&lt;</code> arrive, <code>~</code> transit, <code>@</code> stay, <code>*</code> meal, <code>+</code> visit, <code>?</code> explore. Indented <code>Key: value</code> lines are details. Days show counts, distance in time, and weather; stops show gaps; <code>Cancel by</code> shows deadlines; mismatched weekdays and out-of-order stops are diagnostics.",
      ] },
    ],
  },
  {
    title: "Reading",
    intro: "State is visible without dialogs.",
    chapters: [
      { file: "20-charts.wtf", title: "Charts", prose: [
        "Gauges and sparklines are Unicode blocks, so they render anywhere: countdowns and checklists carry a gauge, numeric columns and sums a sparkline.",
      ] },
      { file: "25-highlighting.wtf", title: "Highlighting", prose: [
        "Colors come from the engine, so what is colored is exactly what is understood. Declarations are bold; references, functions, columns, and stop kinds each have a color; prose dates, money, and durations are recognized without becoming names.",
      ] },
    ],
  },
  {
    title: "Editing",
    intro: "WTF is a language server, so editor features come from the same engine. This page shows the notes; the actions live in your editor.",
    chapters: [
      { file: "21-format-on-type.wtf", title: "Format on Type", prose: [
        "The closing pipe of a row realigns the table; Enter after a checkbox continues the list, and Enter on an empty one ends it. Both work here.",
      ] },
      { file: "22-refactors.wtf", title: "Refactors", prose: [
        "Extract a literal into a named value, inline or freeze a calculation, freeze a relative date, fix a misspelled name.",
      ] },
      { file: "23-rename-and-navigation.wtf", title: "Rename and Navigation", prose: [
        "Rename follows every reference across notes; go to definition, find references, highlights, and the outline share the same names.",
      ] },
      { file: "24-dependency-graph.wtf", title: "Dependency Graph", prose: [
        "Every value and task is a node. Call hierarchy shows what a name reads and what reads it, including <code>@after</code>, subtasks, checklists, and plan constants.",
      ] },
      { file: "31-completion-and-signatures.wtf", title: "Completion", prose: [
        "Completion follows context: columns inside a sum, timers after <code>@timer(</code>, dates after <code>@due(</code>, properties after a dot, stop kinds after a time. Signature help describes each argument.",
      ] },
      { file: "32-folding-and-outline.wtf", title: "Folding and Outline", prose: [
        "Sections, tables, plans, days, stops, and comment blocks fold. The outline lists sections, tasks, values, tables, and columns with their current values.",
      ] },
      { file: "30-diagnostics.wtf", title: "Diagnostics", prose: [
        "Problems point at the exact token: unknown names, cycles, invalid dates, mistyped cells. Unfetched data is a warning, not an error. Uncomment a line below to see one.",
      ] },
    ],
  },
  {
    title: "Workspace",
    intro: "Names are local to a note; imports connect notes; the command line reads the same language.",
    chapters: [
      { file: "26-comments-and-code.wtf", title: "Comments and Code", prose: [
        "HTML comments, fenced code, and inline code are never parsed, so they are the place for examples that should not run.",
      ] },
      { file: "27-cross-note-values.wtf", title: "Other Notes", prose: [
        "<code>source := import(\"./28-cross-note-source.wtf\")</code>, then <code>source.shared_rate</code>. Opening a note never leaks its names into another.",
      ], companion: "28-cross-note-source.wtf" },
      { file: "29-agenda-and-cli.wtf", title: "Command Line", prose: [
        "<code>wtf query file.wtf 'tasks'</code> reads one note, <code>--workspace</code> every note; queries use the note language (<code>filter(tasks, fn(t) =&gt; !t.done)</code>) or pipelines, return <code>--json</code>, and are read-only. <code>wtf ast</code>, <code>wtf graph</code>, <code>wtf refresh</code>, and <code>--fail-on-match</code> cover syntax, dependencies, caches, and CI. The query console in this app runs the same queries.",
      ] },
    ],
  },
];
