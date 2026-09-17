// The book's outline: parts, chapters, and the prose around each example.
// Each chapter names one note from examples/; build.mjs inlines its text.
export const parts = [
  {
    title: "Getting Started",
    intro: "Jot is plain text that can count. You write notes the way you already do, and the parts that look like values start to behave like values. This part introduces the four ideas everything else builds on: literals with names, calculations, types, and dates.",
    chapters: [
      {
        file: "01-values.jot",
        title: "Naming Values",
        prose: [
          "Every Jot program starts with a value you already had to write down anyway. A dollar amount in a sentence, a date, a count of something. To make it available elsewhere, follow it with a colon and a name: <code>[$3,000]:budget</code>. The brackets say \"this is a value\", the name says what to call it.",
          "Once a value has a name, <code>[budget]</code> reads it anywhere, in this note or any other note in the workspace. The editor shows the current value right after the reference, so a sentence like \"we have [remaining] left\" always reads with the number filled in.",
          "Try editing the amount spent below. Every reference updates as you type, because they are reading the value rather than copying it.",
        ],
      },
      {
        file: "02-calculations.jot",
        title: "Calculations",
        prose: [
          "A calculation is a name defined with <code>:=</code> and an expression. Expressions use the operators you expect, and they can read any named value or other calculation. Jot keeps track of what depends on what, so changing one number updates everything downstream.",
          "The result appears as an inlay after the definition. If you hover a calculated name, Jot shows the expression with every input substituted, then the result, then links to the inputs. It is a small thing, but it means a note never hides how a number was made.",
          "A calculation does not need a name. Brackets around an expression in prose, such as <code>[remaining / budget]</code>, show the result right there, and hovering the brackets shows how it was computed. A line that is nothing but math, with each variable in brackets, works too: <code>[subtotal] + [tax]</code> on its own line shows its result at the end, the way a scratchpad would.",
        ],
      },
      {
        file: "03-types.jot",
        title: "Types",
        prose: [
          "Jot literals carry a type, and arithmetic respects it. Money times a number is money. A ratio scales anything. A date plus a duration is a date, and two dates subtract to a duration. Comparisons produce booleans.",
          "Types exist to catch the mistakes that spreadsheets let through. Multiplying two amounts of money is an error, not a very large number. The error points at the exact operator, so you can see which operand is the wrong shape.",
        ],
      },
      {
        file: "04-dates.jot",
        title: "Dates and Relative Dates",
        prose: [
          "Dates are written in ISO form, <code>2026-11-20</code>, and <code>today()</code> is always the current day, so a calculation like <code>departure - today()</code> counts down on its own. Task attributes accept relative dates such as <code>tomorrow</code> or <code>next Friday</code>, and <code>date(\"next Friday\")</code> turns the same words into a value.",
          "Relative dates stay relative: a task due tomorrow is due tomorrow again tomorrow. When you want to pin one down, the Freeze relative date action rewrites it as the concrete date it meant at that moment.",
        ],
      },
      {
        file: "05-currencies.jot",
        title: "Currencies",
        prose: [
          "Money carries its currency. A symbol before the number (<code>$</code>, <code>€</code>, <code>£</code>, <code>¥</code>) or a code after it (<code>700 MXN</code>) both work. Two amounts in different currencies never add up silently; the error tells you to convert one side.",
          "<code>to(hotel, USD)</code> converts using a cached exchange rate, and <code>rate(EUR, USD)</code> reads the rate itself. Rates come from a cache that only <code>jot refresh</code> fills, so nothing is fetched while you type. This page has no cache, which is why the conversions below show warnings; in the native app they show numbers.",
        ],
      },
    ],
  },
  {
    title: "Getting Things Done",
    intro: "Notes are where tasks live, so Jot understands checklists, due dates, dependencies, recurrence, appointments, and timers. None of it needs a new syntax beyond a checkbox and a few attributes.",
    chapters: [
      {
        file: "06-checklists.jot",
        title: "Checklists",
        prose: [
          "A Markdown task list is already a checklist. Give the heading above it a name and the list becomes a value: <code>completed(launch)</code>, <code>remaining(launch)</code>, and <code>total(launch)</code> count leaf tasks, and <code>effort(launch)</code> adds up the estimates of what is left.",
          "Named tasks are booleans, which lets a calculation depend on whether something has been done. Tick a box below and watch the heading's progress gauge and the numbers change together.",
        ],
      },
      {
        file: "07-task-attributes.jot",
        title: "Task Attributes",
        prose: [
          "Attributes trail a task as <code>@key(value)</code>. <code>@due</code> and <code>@scheduled</code> take dates or relative dates, <code>@estimate</code> takes a duration, <code>@tag</code> and <code>#hashtags</code> label a task for filtering, and <code>@after</code> makes it wait for something else. A line with <code>@at</code> and a time is an appointment rather than a task.",
          "The inlay after each task summarizes what matters today: how far away the deadline is, what the task is waiting on, and its estimate. The agenda on the command line reads the same attributes.",
        ],
      },
      {
        file: "08-dependencies.jot",
        title: "Dependencies",
        prose: [
          "<code>@after</code> blocks a task until every dependency is satisfied. A dependency can be a named task, a whole named checklist, or any boolean expression, so \"ship once the budget is under control\" is a single attribute.",
          "Blocked tasks say what they are waiting for. If two tasks end up waiting on each other, the error names the cycle and points at both locations.",
        ],
      },
      {
        file: "09-recurring.jot",
        title: "Recurring Tasks",
        prose: [
          "<code>@every</code> repeats a task by day, week, month, year, or a whole-day duration such as <code>2w</code>. Completing a recurring task does not tick it off for good; it advances the due date and records the completion, so the task is ready for next time. Month repeats keep the day of the month.",
        ],
      },
      {
        file: "10-events.jot",
        title: "Appointments",
        prose: [
          "A line with <code>@at</code> is an appointment. Write the time with an explicit offset so it means the same thing wherever you open the note. Appointments show up in <code>jot today</code> and <code>jot agenda</code>, and a timestamp is also a value you can subtract from <code>now()</code> to count down to it.",
        ],
      },
      {
        file: "11-timers.jot",
        title: "Timers",
        prose: [
          "A countdown or stopwatch is a value defined with <code>countdown(25m)</code> or <code>stopwatch()</code>. Its inlay ticks while it runs, and properties such as <code>.remaining</code> and <code>.running</code> feed other calculations. Attach one to a task with <code>@timer</code>.",
          "Starting, pausing, and resetting are code actions that rewrite the definition with the elapsed time and a timestamp. Ticking never edits the note; only your actions do, which keeps the file safe to sync and diff.",
        ],
      },
    ],
  },
  {
    title: "Tables and Optimization",
    intro: "Tables give notes structured data with types. From there it is a short step to asking the note to decide something: a linear plan chooses the best values under constraints, decision columns pick rows, and goal seek runs a calculation backwards.",
    chapters: [
      {
        file: "12-tables.jot",
        title: "Tables",
        prose: [
          "A table is a named value followed by a Markdown table. Column names become identifiers, every column keeps one type, and <code>sum(table, expression)</code> evaluates the expression for each row, with column names standing for that row's cells.",
          "Renaming a column updates the formulas that use it, a bad cell is reported at the cell, and Format Document aligns the pipes. Hover a numeric column for a sparkline of its values.",
        ],
      },
      {
        file: "13-calculated-cells.jot",
        title: "Calculated Cells",
        prose: [
          "A cell in brackets is a calculation, following the same convention as brackets in prose. It can read any named value from any note, so a table can be driven by numbers defined elsewhere. Each calculated cell shows its value as an inlay.",
        ],
      },
      {
        file: "14-plans.jot",
        title: "Linear Plans",
        prose: [
          "A plan is a calculation that chooses values. <code>maximize</code> or <code>minimize</code> an expression, then list constraints in a two-column table. Names that no note defines become decision variables; every other name is a constant read from your notes, so the plan re-solves as the numbers around it change.",
          "Each constraint row shows how much of its limit is used and whether it binds. A plan that cannot be satisfied, or that could grow without limit, is a diagnostic on the objective. The solver is pure Rust, which is why it runs right here in the page.",
        ],
      },
      {
        file: "15-decision-columns.jot",
        title: "Decision Columns",
        prose: [
          "A table column named <code>take?</code> is a yes-or-no choice for each row, and <code>servings#</code> is a whole number. A plan that sums over such a column decides every row, which turns a packing list or a menu into a small optimization problem.",
          "Each decided cell shows its choice as an inlay, and a code action on the plan line writes the choices into the table when you want them on the page.",
        ],
      },
      {
        file: "16-goal-seek.jot",
        title: "Goal Seek",
        prose: [
          "<code>solve(constraint)</code> makes the definition's own name the unknown. Jot follows the chain of calculations that mention it and finds the boundary value where the constraint holds. The unit comes from the chain: money in, money out.",
          "Linear goal seeks have a closed form, so no solver runs; the answer is exact.",
        ],
      },
    ],
  },
  {
    title: "The World Outside",
    intro: "Some numbers live elsewhere: the status of a pull request, an exchange rate, tomorrow's weather. Jot brings them in through a cache that you refresh on purpose, so a note never phones out while you type and keeps working offline.",
    chapters: [
      {
        file: "17-resources.jot",
        title: "Resources and Links",
        prose: [
          "Links, files, places, and GitHub items are values. Any of them can be named and referenced, and the editor offers an Open lens on each. GitHub pull requests, issues, and commits get a status badge once refreshed, and properties such as <code>.merged</code> and <code>.checks_passed</code> feed calculations and dependencies.",
        ],
      },
      {
        file: "18-lookups.jot",
        title: "Weather and Quotes",
        prose: [
          "<code>forecast(place, date)</code> is a day's weather with a high, a low, a summary, and a chance of rain; add <code>F</code> for Fahrenheit. <code>quote(NVDA)</code> is the last price as money. Uppercase names such as <code>NVDA</code> are codes rather than references.",
          "Hover a value to see every lookup it used, how old the data is, and where it came from. Providers are keyless by default and replaceable with your own commands.",
        ],
      },
    ],
  },
  {
    title: "Trips",
    intro: "An itinerary is prose with a shape: days, times, and details. Jot recognizes that shape without asking you to change how you write it.",
    chapters: [
      {
        file: "19-itinerary.jot",
        title: "Itineraries",
        prose: [
          "A line with a weekday, month, and day starts a day. A line starting with a clock time is a stop, and one ASCII marker after the time names its kind: <code>&gt;</code> depart, <code>&lt;</code> arrive, <code>~</code> transit, <code>@</code> stay, <code>*</code> meal, <code>+</code> visit, <code>?</code> explore. Indented <code>Key: value</code> lines are the stop's details.",
          "Each kind has its own color, so a day scans before you read it. Days show their stop count, how far away they are, and the weather once refreshed; stops show the gap to the next one; <code>Cancel by</code> details show the deadline; addresses open in Maps. Diagnostics catch a weekday that does not match its date and stops out of order.",
        ],
      },
    ],
  },
  {
    title: "Reading at a Glance",
    intro: "Most of what Jot adds to a note is meant to be read, not written. Highlighting and small charts make the state of a note visible without opening a dialog.",
    chapters: [
      {
        file: "20-charts.jot",
        title: "Plain-Text Charts",
        prose: [
          "Gauges and sparklines are drawn with Unicode block characters, so they render in any editor with no image support. Countdowns and checklists carry a gauge in their inlay, hovers add the percentage, and numeric columns and sums show a sparkline with their range.",
        ],
      },
      {
        file: "25-highlighting.jot",
        title: "Highlighting",
        prose: [
          "The engine produces the highlighting, not a second grammar in the editor, so what is colored is exactly what is understood. Prose values such as dates, times, money, and durations are recognized without becoming symbols. Declared names are bold; references, functions, columns, and itinerary kinds each have their own color.",
        ],
      },
    ],
  },
  {
    title: "Editing",
    intro: "Jot is a language server first, so the usual editor features come from the same engine: completion, signature help, rename, go to definition, folding, formatting, refactors, and a dependency graph. This page can show the notes and their live state; the actions themselves belong to your editor.",
    chapters: [
      {
        file: "21-format-on-type.jot",
        title: "Format on Type",
        prose: [
          "Typing the closing pipe of a table row realigns the whole table, and only the row you are typing is touched once every column has a cell. Enter after a checkbox continues the list with the same indent, and Enter on an empty checkbox ends it. Both work in this page's blocks too.",
        ],
      },
      {
        file: "22-refactors.jot",
        title: "Code Actions and Refactors",
        prose: [
          "Select a literal in prose and extract it into a named value. Inline a calculation into the place it is used, or freeze it at its current value. Freeze a relative date. A misspelled name gets a quick fix suggesting the closest one. These are code actions in Zed and VS Code.",
        ],
      },
      {
        file: "23-rename-and-navigation.jot",
        title: "Rename, References, and Navigation",
        prose: [
          "Rename a value and every reference follows, across notes. Go to definition, find references, and document highlights all understand the same names, and the outline lists every value, task, and table in the note.",
        ],
      },
      {
        file: "24-dependency-graph.jot",
        title: "The Dependency Graph",
        prose: [
          "Every value and task is a node in a graph. Show Call Hierarchy on a name: outgoing calls are what it reads, incoming calls are what reads it. Tasks link through <code>@after</code>, subtasks, and checklists, and plans link to their constants and variables.",
        ],
      },
      {
        file: "31-completion-and-signatures.jot",
        title: "Completion and Signature Help",
        prose: [
          "Completion knows the context: names in prose, column names inside a sum, only timers after <code>@timer(</code>, only dates after <code>@due(</code>, properties after a dot, stop kinds after a time. Signature help describes each argument of a function as you type it.",
        ],
      },
      {
        file: "32-folding-and-outline.jot",
        title: "Folding and Outline",
        prose: [
          "Sections, tables, plans, itinerary days, stops, and comment blocks fold. The outline shows sections, tasks, values, tables, and columns in source order, with each item's current value beside it.",
        ],
      },
      {
        file: "30-diagnostics.jot",
        title: "Diagnostics",
        prose: [
          "Problems point at the exact token: an unknown name, a cycle with both locations, an invalid date, a cell of the wrong type. Unfetched data and unrecognized itinerary stops are warnings rather than errors, so a note that is merely waiting on a refresh still passes <code>jot check</code>.",
          "The example below is valid as written; uncomment one of the suggested lines to see the diagnostic appear.",
        ],
      },
    ],
  },
  {
    title: "The Workspace",
    intro: "A workspace is a folder of notes. Names resolve across all of them, comments and code stay inert, and the command line reads the same files the editor does.",
    chapters: [
      {
        file: "26-comments-and-code.jot",
        title: "Comments and Code Stay Inert",
        prose: [
          "HTML comments, fenced code blocks, and inline code are never parsed as definitions, tasks, or attributes. That makes them the place to leave instructions or examples that should not run.",
        ],
      },
      {
        file: "27-cross-note-values.jot",
        title: "Values From Other Notes",
        prose: [
          "Names resolve across every note in the workspace, and a name defined in the current note wins a tie. The two notes below share a workspace on this page, so the first reads values defined in the second.",
        ],
        companion: "28-cross-note-source.jot",
      },
      {
        file: "29-agenda-and-cli.jot",
        title: "Agenda and the Command Line",
        prose: [
          "<code>jot today</code>, <code>jot agenda --week</code>, and <code>jot tasks</code> read every note. <code>jot capture</code> appends a task, <code>jot complete</code> ticks one, <code>jot check</code> reports problems, <code>jot plan</code> solves a plan, and <code>jot refresh</code> fills the caches. Add <code>--json</code> to any listing.",
        ],
      },
      {
        file: "33-images-and-files.jot",
        title: "Images and Files",
        prose: [
          "Relative paths resolve from the note, and home-relative paths work too. Images preview on hover and open with a lens; other files simply open.",
        ],
      },
    ],
  },
];
