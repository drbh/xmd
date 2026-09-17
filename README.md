# Jot

Plain-text notes with calculated values, checklists, timers, reusable links, and dates.
The same Rust engine powers the Zed language server and terminal commands.

## Try it

```sh
cargo build
./target/debug/jot today
./target/debug/jot agenda --week
./target/debug/jot tasks --tag errands --json
./target/debug/jot check
```

Open `jots/daily.jot`, `jots/resources.jot`, or `jots/timers.jot` in Zed. If you already installed the dev
extension, restart the Jot language server after rebuilding the binary. The
extension uses `target/debug/jot` in the project root. To install it initially,
use **Install Dev Extension** and choose `zed-extension`.

For notes in other projects, set `lsp.jot.binary.path` in your Zed user settings
to this repository's absolute `target/debug/jot` path and `arguments` to `["lsp"]`.
The extension honors that override and otherwise uses the project's debug build.

Enable these settings for the Jot language:

```json
{
  "languages": {
    "Jot": {
      "semantic_tokens": "full",
      "inlay_hints": { "enabled": true, "show_other_hints": true }
    }
  }
}
```

Rebuild the dev extension after changing its language configuration or token
rules. On macOS, `/usr/bin/jot` is an unrelated command; the examples deliberately
use `./target/debug/jot` or `cargo run -- ...`.

## Checklists and calculations

```text
## Release :release
- [x] Write parser
- [ ] Review implementation :review @estimate(30m)
- [ ] Publish package @after(review) @estimate(10m)

[progress] := completed(release) / total(release)
[remaining_work] := effort(release)
```

Use Zed's code actions on a task to **Complete task** or **Reopen task**. After
rebuilding the dev extension, pressing Enter at the end of a checkbox line
continues the checklist on the next line. Completing a parent updates its
descendants. A parent's computed completion follows its
children; manually editing `[x]` remains possible. Actions refuse blocked tasks,
but there is no restriction on editing your own source text.

Headings display completion counts and estimated work left. Functions `total`,
`completed`, `remaining`, and `effort` accept a named heading. They count leaf
tasks under that heading (including child headings), without counting parents
twice. `effort` sums estimates only for unfinished leaves; an absent estimate
contributes zero. Empty-checklist division produces a diagnostic.

Names use ASCII letters, digits, and underscores, beginning with a letter or `_`.
A heading or task's optional `:name` goes after its title and before metadata.
Dependencies use `@after(review)` or `@after(review, other_task)`; a named
checklist or boolean expression can also be a dependency. Dependency cycles and
unknown names produce diagnostics.

Literals retain the original `[value]:name` syntax. Derived definitions use
`[name] := expression`. Multiple literals per prose line work. Arithmetic
supports precedence, parentheses, unary signs, comparisons, `&&`, `||`, and `!`.
Money retains its currency formatting; money/money and count/count ratios show
percentages. Ordinary numeric division stays numeric. Durations use `s`, `m`, `h`,
`d`, or `w` and are stored as whole seconds. Fractional units such as `0.5m` are
accepted when they resolve to whole seconds. Strings use double quotes inside
expressions. Markdown links, images, inline code, fenced code, and HTML comments
are distinguished from Jot names. Example syntax inside code/comments is inert.

## Stopwatches and countdowns

```text
[focus] := countdown(25m)
[debugging] := stopwatch()

- [ ] Investigate flaky test @timer(debugging) @estimate(30m)

Time left: [focus.remaining].
Time spent: [debugging.elapsed].
[over_estimate] := debugging.elapsed > 30m
```

Timers start **idle**. Put the cursor on a declaration, a reference, or an
associated checklist line and open code actions. **Start timer**, **Pause timer**,
**Resume timer**, and **Reset timer** appear when applicable. Start/resume capture
the time when the action is executed. Reset returns to idle and clears elapsed
time; use the editor's undo to undo a timer action. Completing a checklist task
does not automatically stop its timer. `@timer` accepts a named timer, including
an alias or a unique definition in another note.

Running hints refresh approximately once per second in open notes. Zed controls
the final rendering cadence. Ticking never edits a file, republishes highlighting,
or sends refreshes once no open note depends on a running timer or `now()`.
Countdowns stop at zero: `.done` becomes true, `.running` becomes false, and
`.elapsed` is capped at the countdown duration. Expiry is visual for now; there
is no audible alarm, OS notification, or background service.

Timer properties are `.elapsed`, `.running`, `.done`, and `.state` (`"idle"`,
`"running"`, `"paused"`, `"done"`). Countdowns additionally expose `.remaining`
and `.duration`; a stopwatch's `.done` is always false. Duration properties work
in arithmetic and comparisons. `[name.property]` works in prose with inlays,
hover, completion, navigation, and rename of the underlying name.

Actions persist state in the expression itself; save the note to preserve it
across restarts. The optional arguments are elapsed duration, then the timestamp
at which the current running segment began:

```text
[focus] := countdown(25m, 0s, 2026-09-16T14:00:00-04:00)
[debugging] := stopwatch(73s)
```

Here the countdown is running (or done, depending on the current time), and the
stopwatch is paused at 73 seconds. With a timestamp, elapsed time is accumulated
time plus time since that timestamp; without one, elapsed time is frozen. This
keeps notes self-contained and makes sleep/closed-editor time count while a timer
is running. There is no hidden timer cache. Controls preserve the countdown's
original duration expression, even when it refers to another value.

`now()` provides a live timestamp, sampled once per evaluation. For example,
`[until_meeting] := meeting_at - now()` is a signed duration until an appointment.
Use timer actions to capture a start time; writing `stopwatch(0s, now())` would
continually move the start to the present. Timers use wall-clock timestamps, so
system clock adjustments affect them (negative running intervals clamp to zero).
They have whole-second resolution; pausing discards a fractional second and is
not intended for precision profiling.

## Reusable resources and workspace names

```text
[https://github.com/owner/repo/pull/42]:parser_pr
[./assets/receipt.jpg]:receipt
[geo:40.7306,-73.9866]:cafe

- [ ] Review [parser_pr]
- [ ] Expense lunch using [receipt]
Meet at [cafe].
```

Use hover for details and image previews where the editor supports images in
Markdown hovers; use document links to open files, URLs, or the map. Image bytes
stay in their original files. Relative paths are resolved against the defining
note, even when a reference is used in another folder. Normal Markdown links
and images are clickable too.

The workspace indexes `.jot` files recursively, respects ignore files, and skips
hidden/build directories and symlinks. Open editor buffers override disk content.
Names resolve in the current file first, then to a unique workspace definition.
Ambiguous names are reported; there are no implicit namespace guesses. Completion,
go-to-definition, find references, and rename work across notes, including task
dependencies and date formulas. Duplicate target names are rejected by rename.

### GitHub status

Install/authenticate the GitHub CLI (`gh`), then run:

```sh
./target/debug/jot refresh
```

Or use **Refresh GitHub resources** from Zed's code actions. This only reads
GitHub; it never merges PRs or changes issues. It supports GitHub.com PRs, issues,
and commits. PRs expose title, state, merged status, reviews, and check summaries.
Links show the cache timestamp. Failed refreshes preserve the previous cache;
missing checks are unknown, not passing. The ignored `.jot/cache.json` contains
cached metadata and timestamps, never credentials. Remote status does not update
until you explicitly refresh it again.

```text
[ready_to_ship] := parser_pr.merged && parser_pr.checks_passed
```

Properties include `.merged`, `.checks_passed`, `.state`, `.title`, `.url`, and
`.exists` (local files). Remote properties produce a diagnostic until their
metadata is available. A cached status is an observation, not a deployment gate.

## Dates, recurrence, and the agenda

```text
[2026-09-25]:departure
[days_left] := departure - today()

- [ ] Book hotel @due(departure - 7d)
- [ ] Renew prescription @scheduled(2026-09-17) @due(2026-09-20)
- [ ] Review finances @every(month) @due(2026-09-30)
- Meet at [cafe] @at(2026-09-18T14:00-04:00)
```

`@due` is a deadline; `@scheduled` is a planned work date; `@at` is an appointment.
Dates use ISO `YYYY-MM-DD`. Timestamps accept ISO times with an explicit offset,
or local `YYYY-MM-DDTHH:MM` (ambiguous daylight-saving times require an offset).
Date-only arithmetic requires whole days; timestamps can add hours, minutes, or seconds.
`today()` is deliberately dynamic and editor hints refresh at midnight.

Relative input supports `today`, `tomorrow`, `yesterday`, and `next Friday` (and
other weekdays). Capture converts these to fixed dates. When typing directly in
Zed, use **Resolve relative dates to calendar dates** to freeze them; until then,
relative text is evaluated against the current day. A weekday means its next
occurrence, strictly after today. `date("next Friday")` is an explicitly dynamic
expression.

Recurrence supports `day`, `week`, `month`, `year`, or whole-day durations such
as `2w`. Complete it through a code action or `jot complete`: the task remains
unchecked, its due date advances past the completed occurrence, and an HTML
`jot-history` comment records the title, completion date, original due date, and
next date. `@repeat_from` preserves the recurrence anchor, including month-end
behavior (January 31 → February 28 → March 31). Missed instances advance to the
next future occurrence rather than creating a backlog. Recurrence on parent tasks
is rejected; complete recurring leaves individually. Hand-editing `[x]` does not
run recurrence/history actions.

```sh
./target/debug/jot today
./target/debug/jot agenda --week
./target/debug/jot tasks --tag errands --json
./target/debug/jot tasks --all
./target/debug/jot capture "Call the dentist" --due "next Friday" --tag errands
./target/debug/jot capture "Review notes" --journal
./target/debug/jot complete inbox.jot:1
```

All commands accept `--root PATH`. Agenda/task commands also accept `--on
YYYY-MM-DD` for reproducible queries; capture/complete accept it for backdated
entries. Task tags use `#errands` or `@tag(errands)`.

Today includes unfinished tasks that are overdue, due, scheduled, or undated,
and today's appointments. The week view covers today through six days from now.
`tasks` lists all unfinished leaves, including future tasks; `--all` includes
completed leaves. Results contain source paths and one-based line numbers; JSON
includes URI, dates, tags, estimates, blocked reasons, and evaluation errors.
JSON includes exact `estimate_seconds` and `estimate_minutes` (which can be fractional).

**Show today's agenda** in Zed opens a generated `.jot/today.md` with links back
to source notes. Re-run the action to refresh the view. Capture appends to
`inbox.jot`, `--journal` uses `journal/YYYY-MM-DD.jot`, and `--file` chooses a note.
CLI commands read saved files, so save editor changes before completing tasks
from the terminal. Editor actions use versioned edits against the live buffer.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo build
```

With no subcommand (or `lsp`), Jot speaks standard LSP on stdio. Do not run that
process separately when using Zed; the editor starts and owns the connection.
