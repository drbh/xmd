//! Signature help, over the one table of built-in calls and task attributes
//! that also feeds completion.
use crate::{completion::call_context, hover::markup, intelligence_impl::inert};
use Outcome::{Kind, Words};
use eval::engine::{Builtin, ValueType};
use lsp_types::*;
use model::{Document, byte_at};

/// What a call answers with: one value kind wherever the answer has one, so
/// the table cannot invent a type name, and prose for the unions and for the
/// attributes that produce no value at all.
#[derive(Clone, Copy)]
pub(crate) enum Outcome {
    Kind(ValueType),
    Words(&'static str),
}
impl Outcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Kind(kind) => kind.as_str(),
            Self::Words(words) => words,
        }
    }
}
/// The section a built-in belongs to, so the reference and the docs can group
/// the table the same way without a second list of names.
#[derive(Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::VariantArray)]
pub(crate) enum Group {
    Values,
    Text,
    Lists,
    Numbers,
    Dates,
    Tasks,
    Timers,
    Lookups,
    Plans,
    Modules,
    Control,
}
impl Group {
    /// Declaration order is the order the reference prints the sections in.
    pub(crate) const ALL: &'static [Group] = <Self as strum::VariantArray>::VARIANTS;
    pub(crate) fn as_str(self) -> &'static str {
        self.into()
    }
}

// `Tier` and the per-built-in tier data live in `evaluate::engine`, since the
// engine itself needs to know which built-ins module code alone may call;
// this table only reads the answer back for signature help and the reference.
pub(crate) use eval::engine::Tier;

/// One built-in call or task attribute, as signature help and completion show
/// it. The table below is the only description of them the editor has: the
/// reference model is generated from it, so `documentation`, `example` (what
/// signature help fills in) and `try_` (a complete note a reader can paste)
/// are written here and nowhere else.
#[derive(Clone, Copy)]
pub(crate) struct Signature {
    pub(crate) name: &'static str,
    pub(crate) params: &'static [&'static str],
    pub(crate) result: Outcome,
    pub(crate) documentation: &'static str,
    pub(crate) example: &'static str,
    pub(crate) tier: Tier,
    pub(crate) group: Group,
    pub(crate) try_: &'static str,
}
pub(crate) fn is_builtin_function(name: &str) -> bool {
    eval::engine::is_builtin_function(name)
}

/// Every built-in has exactly one description, and the compiler checks it: a
/// new `Builtin` variant does not compile until this match answers for it.
const fn describe(builtin: Builtin) -> Signature {
    let name = builtin.as_str();
    let tier = eval::engine::tier(builtin);
    match builtin {
        Builtin::Import => Signature {
            name,
            params: &["id: Text"],
            result: Kind(ValueType::Record),
            documentation: "Load a module by ID, or a note by path such as import(\"./values.wtf\"); members keep their own source.",
            example: "\"format\"",
            tier,
            group: Group::Modules,
            try_: "units := import(\"units\")\nmiles := units.convert(100, \"km\", \"mi\")",
        },
        Builtin::SolveLinear => Signature {
            name,
            params: &["model: Record"],
            result: Kind(ValueType::Record),
            documentation: "Solve a bounded linear model and return raw numeric values and status.",
            example: "model",
            tier,
            group: Group::Plans,
            try_: "best := solve_linear({goal: \"maximize\", variables: {x: {kind: \"integer\", lower: 0, upper: 3}}, objective: {constant: 0, terms: {x: 1}}, constraints: []})\nstatus := best.status",
        },
        Builtin::Object => Signature {
            name,
            params: &["entries: List"],
            result: Kind(ValueType::Record),
            documentation: "Build a record from key/value pairs; duplicate keys are rejected.",
            example: "[{key: \"x\", value: 1}]",
            tier,
            group: Group::Values,
            try_: "pairs := [{key: \"city\", value: \"Oaxaca\"}, {key: \"nights\", value: 3}]\ntrip := object(pairs)",
        },
        Builtin::ParseDate => Signature {
            name,
            params: &["text: Text", "format: Text"],
            result: Words("Date or Null"),
            documentation: "Parse a calendar date with a strftime format; invalid input returns null.",
            example: "\"2026-09-18\", \"%F\"",
            tier,
            group: Group::Dates,
            try_: "departure := parse_date(\"2026-11-20\", \"%F\")",
        },
        Builtin::ParseDatetime => Signature {
            name,
            params: &["text: Text", "format: Text", "offset: DateTime"],
            result: Words("DateTime or Null"),
            documentation: "Parse a local timestamp using a reference timestamp's offset.",
            example: "\"2026-09-18 09:30\", \"%F %H:%M\", now()",
            tier,
            group: Group::Dates,
            try_: "landing := parse_datetime(\"2026-11-20 09:30\", \"%F %H:%M\", now())",
        },
        Builtin::Entries => Signature {
            name,
            params: &["record: Record"],
            result: Kind(ValueType::List),
            documentation: "List key/value pairs in key order.",
            example: "{x: 1}",
            tier,
            group: Group::Values,
            try_: "nights := {oaxaca: 3, mexico_city: 2}\npairs := entries(nights)",
        },
        Builtin::Number => Signature {
            name,
            params: &["value: Number, Money, Ratio, Duration or Count"],
            result: Kind(ValueType::Number),
            documentation: "Extract the numeric magnitude; durations use seconds.",
            example: "90m",
            tier,
            group: Group::Values,
            try_: "slot := 90m\nseconds := number(slot)",
        },
        Builtin::Source => Signature {
            name,
            params: &["value: Any"],
            result: Kind(ValueType::Text),
            documentation: "Format a typed scalar as a round-trippable expression.",
            example: "now()",
            tier,
            group: Group::Values,
            try_: "departure := today() + 7d\nliteral := source(departure)",
        },
        Builtin::MakeDate => Signature {
            name,
            params: &["year: Number", "month: Number", "day: Number"],
            result: Words("Date or Null"),
            documentation: "Construct a calendar date; invalid dates return null.",
            example: "2026, 9, 18",
            tier,
            group: Group::Dates,
            try_: "departure := make_date(2026, 11, 20)",
        },
        Builtin::DurationParts => Signature {
            name,
            params: &["duration: Duration"],
            result: Kind(ValueType::Record),
            documentation: "Split integer seconds into total hours, remaining minutes and seconds without rounding.",
            example: "90m",
            tier,
            group: Group::Dates,
            try_: "parts := duration_parts(90m)\nminutes := parts.minutes",
        },
        Builtin::DateParts => Signature {
            name,
            params: &["date: Date or DateTime"],
            result: Kind(ValueType::Record),
            documentation: "Read year, month, day and weekday (Monday is zero).",
            example: "today()",
            tier,
            group: Group::Dates,
            try_: "parts := date_parts(today())\nyear := parts.year",
        },
        Builtin::AtTime => Signature {
            name,
            params: &["date: Date", "time: Duration", "offset: DateTime"],
            result: Kind(ValueType::DateTime),
            documentation: "Combine a date and time of day using the reference timestamp offset.",
            example: "today(), 9h, now()",
            tier,
            group: Group::Dates,
            try_: "standup := at_time(today(), 9h, now())",
        },
        Builtin::ParseTime => Signature {
            name,
            params: &["text: Text", "format: Text"],
            result: Words("Duration or Null"),
            documentation: "Parse a time of day as seconds since midnight.",
            example: "\"09:30\", \"%H:%M\"",
            tier,
            group: Group::Dates,
            try_: "opens := parse_time(\"09:30\", \"%H:%M\")",
        },
        Builtin::ParseDuration => Signature {
            name,
            params: &["text: Text"],
            result: Words("Duration or Null"),
            documentation: "Parse a written duration.",
            example: "\"2h\"",
            tier,
            group: Group::Dates,
            try_: "slot := parse_duration(\"2h\")",
        },
        Builtin::PadStart => Signature {
            name,
            params: &["text: Text", "width: Number", "fill: Text"],
            result: Kind(ValueType::Text),
            documentation: "Pad text to a character width with one character.",
            example: "\"3\", 2, \"0\"",
            tier,
            group: Group::Text,
            try_: "minute := pad_start(\"3\", 2, \"0\")",
        },
        Builtin::PadEnd => Signature {
            name,
            params: &["text: Text", "width: Number", "fill: Text"],
            result: Kind(ValueType::Text),
            documentation: "Pad text on the right.",
            example: "\"x\", 3, \" \"",
            tier,
            group: Group::Text,
            try_: "cell := pad_end(\"pear\", 8, \" \")",
        },
        Builtin::Slice => Signature {
            name,
            params: &["value: Text or List", "start: Number", "end: Number"],
            result: Words("Text or List"),
            documentation: "Take a half-open range; text indices count Unicode characters.",
            example: "\"hello\", 0, 2",
            tier,
            group: Group::Lists,
            try_: "code := slice(\"OAXACA\", 0, 3)",
        },
        Builtin::Concat => Signature {
            name,
            params: &["lists: List..."],
            result: Kind(ValueType::List),
            documentation: "Concatenate lists.",
            example: "[1, 2], [3]",
            tier,
            group: Group::Lists,
            try_: "legs := concat([\"Oaxaca\", \"Puebla\"], [\"Mexico City\"])",
        },
        Builtin::Trim => Signature {
            name,
            params: &["text: Text"],
            result: Kind(ValueType::Text),
            documentation: "Remove surrounding whitespace.",
            example: "\" hello \"",
            tier,
            group: Group::Text,
            try_: "city := trim(\"  Oaxaca  \")",
        },
        Builtin::Type => Signature {
            name,
            params: &["value: Any"],
            result: Kind(ValueType::Text),
            documentation: "Get the runtime type name.",
            example: "42",
            tier,
            group: Group::Values,
            try_: "price := $12.50\nkind := type(price)",
        },
        Builtin::Floor => Signature {
            name,
            params: &["number: Number"],
            result: Kind(ValueType::Number),
            documentation: "Round down to an integer.",
            example: "1.5",
            tier,
            group: Group::Numbers,
            try_: "nights := floor(2.8)",
        },
        Builtin::Round => Signature {
            name,
            params: &["number: Number"],
            result: Kind(ValueType::Number),
            documentation: "Round to the nearest integer.",
            example: "1.5",
            tier,
            group: Group::Numbers,
            try_: "nights := round(2.8)",
        },
        Builtin::Repeat => Signature {
            name,
            params: &["text: Text", "count: Number"],
            result: Kind(ValueType::Text),
            documentation: "Repeat text a bounded number of times.",
            example: "\"█\", 3",
            tier,
            group: Group::Text,
            try_: "bar := repeat(\"█\", 3)",
        },
        Builtin::FormatDate => Signature {
            name,
            params: &["date: Date or DateTime", "format: Text"],
            result: Kind(ValueType::Text),
            documentation: "Format a date or timestamp with strftime directives.",
            example: "today(), \"%Y-%m-%d\"",
            tier,
            group: Group::Dates,
            try_: "stamp := format_date(today(), \"%Y-%m-%d\")",
        },
        Builtin::Error => Signature {
            name,
            params: &["message: Text"],
            result: Words("Never"),
            documentation: "Return an evaluation error.",
            example: "\"Missing data\"",
            tier,
            group: Group::Control,
            try_: "budget := $500\nchecked := if(budget > $0, budget, error(\"Budget must be positive\"))",
        },
        Builtin::If => Signature {
            name,
            params: &["condition: Boolean", "then: Value", "else: Value"],
            result: Words("Value"),
            documentation: "Evaluate only the selected branch.",
            example: "true, 1, 0",
            tier,
            group: Group::Control,
            try_: "stock := 3\nlabel := if(stock > 0, \"in stock\", \"sold out\")",
        },
        Builtin::Coalesce => Signature {
            name,
            params: &["values: Value..."],
            result: Words("Value"),
            documentation: "Return the first non-null value.",
            example: "null, 1",
            tier,
            group: Group::Control,
            try_: "nickname := null\nname := coalesce(nickname, \"friend\")",
        },
        Builtin::Map => Signature {
            name,
            params: &["items: List", "function: Function"],
            result: Kind(ValueType::List),
            documentation: "Apply a pure function to every item.",
            example: "[1, 2], fn(x) => x * 2",
            tier,
            group: Group::Lists,
            try_: "prices := [$3, $4.50]\ndoubled := map(prices, fn(p) => p * 2)",
        },
        Builtin::Filter => Signature {
            name,
            params: &["items: List", "predicate: Function"],
            result: Kind(ValueType::List),
            documentation: "Keep items whose predicate returns true.",
            example: "[1, 2], fn(x) => x > 1",
            tier,
            group: Group::Lists,
            try_: "sizes := [1, 2, 3]\nbig := filter(sizes, fn(n) => n > 1)",
        },
        Builtin::SortBy => Signature {
            name,
            params: &["items: List", "key: Function"],
            result: Kind(ValueType::List),
            documentation: "Stable ascending sort by a compatible scalar key; nulls come last.",
            example: "[3, 1], fn(x) => x",
            tier,
            group: Group::Lists,
            try_: "cities := [\"Puebla\", \"Oaxaca\"]\nsorted := sort_by(cities, fn(c) => c)",
        },
        Builtin::GroupBy => Signature {
            name,
            params: &["items: List", "key: Function"],
            result: Kind(ValueType::List),
            documentation: "Group by a scalar key into {key, rows} records, in first-seen order.",
            example: "[1, 2, 1], fn(x) => x",
            tier,
            group: Group::Lists,
            try_: "votes := [\"yes\", \"no\", \"yes\"]\ntallies := group_by(votes, fn(v) => v)",
        },
        Builtin::Eval => Signature {
            name,
            params: &["expression: Text"],
            result: Words("Value"),
            documentation: "Evaluate expression text in the current document's scope.",
            example: "\"price * 2\"",
            tier,
            group: Group::Control,
            try_: "price := $12.50\ndoubled := eval(\"price * 2\")",
        },
        Builtin::Fold => Signature {
            name,
            params: &["items: List", "initial: Value", "function: Function"],
            result: Words("Value"),
            documentation: "Combine items left to right with an accumulator.",
            example: "[1, 2], 0, fn(a, x) => a + x",
            tier,
            group: Group::Lists,
            try_: "nights := [2, 3, 1]\ntotal := fold(nights, 0, fn(sum, n) => sum + n)",
        },
        Builtin::Get => Signature {
            name,
            params: &["collection: Record or List", "key: Text or Number"],
            result: Words("Value"),
            documentation: "Read a field or index; return null when absent.",
            example: "{name: \"hello\"}, \"name\"",
            tier,
            group: Group::Values,
            try_: "trip := {city: \"Oaxaca\", nights: 3}\ncity := get(trip, \"city\")",
        },
        Builtin::Length => Signature {
            name,
            params: &["value: List, Record, or Text"],
            result: Kind(ValueType::Count),
            documentation: "Count items, fields, or Unicode characters.",
            example: "\"hello\"",
            tier,
            group: Group::Values,
            try_: "letters := length(\"Oaxaca\")",
        },
        Builtin::Text => Signature {
            name,
            params: &["value: Value"],
            result: Kind(ValueType::Text),
            documentation: "Format a value as text; null remains null.",
            example: "$25",
            tier,
            group: Group::Text,
            try_: "price := $25\nlabel := \"Lunch costs \" + text(price)",
        },
        Builtin::Debug => Signature {
            name,
            params: &["value: Value"],
            result: Kind(ValueType::Text),
            documentation: "Inspect any value as compact JSON text in an inlay. Records, lists, and host objects expose their fields; money, dates, durations, and ratios keep their type and units.",
            example: "{rain: 35%, pack: true}",
            tier,
            group: Group::Values,
            try_: "weather := {high: 25, rain: 35%}\n[debug(weather)]",
        },
        Builtin::Sparkline => Signature {
            name,
            params: &[
                "values: List",
                "min?: Number, Money, Ratio, or Duration",
                "max?: Number, Money, Ratio, or Duration",
            ],
            result: Kind(ValueType::Text),
            documentation: "Draw one Unicode bar per numeric value, in list order. Null leaves a gap (·). The scale uses the data's minimum and maximum unless both bounds are supplied; values outside fixed bounds are clipped. Values and bounds must use matching units.",
            example: "[12, 18, 9, 24]",
            tier,
            group: Group::Values,
            try_: "sales := [12, 18, 9, 24]\n[sparkline(sales)]\n[sparkline([20%, 40%, 30%], 0%, 100%)]",
        },
        Builtin::Contains => Signature {
            name,
            params: &["value: List or Text", "part: Value"],
            result: Kind(ValueType::Boolean),
            documentation: "Test membership or a text substring.",
            example: "\"hello\", \"ell\"",
            tier,
            group: Group::Text,
            try_: "has_ell := contains(\"hello\", \"ell\")",
        },
        Builtin::StartsWith => Signature {
            name,
            params: &["text: Text", "prefix: Text"],
            result: Kind(ValueType::Boolean),
            documentation: "Test a text prefix.",
            example: "\"hello\", \"he\"",
            tier,
            group: Group::Text,
            try_: "local := starts_with(\"+52 951\", \"+52\")",
        },
        Builtin::EndsWith => Signature {
            name,
            params: &["text: Text", "suffix: Text"],
            result: Kind(ValueType::Boolean),
            documentation: "Test a text suffix.",
            example: "\"hello\", \"lo\"",
            tier,
            group: Group::Text,
            try_: "is_note := ends_with(\"trip.wtf\", \".wtf\")",
        },
        Builtin::Split => Signature {
            name,
            params: &["text: Text", "separator: Text"],
            result: Kind(ValueType::List),
            documentation: "Split text into pieces.",
            example: "\"a/b\", \"/\"",
            tier,
            group: Group::Text,
            try_: "parts := split(\"2026-11-20\", \"-\")",
        },
        Builtin::Join => Signature {
            name,
            params: &["items: List", "separator: Text"],
            result: Kind(ValueType::Text),
            documentation: "Join a list of text.",
            example: "[\"a\", \"b\"], \"/\"",
            tier,
            group: Group::Text,
            try_: "route := join([\"Oaxaca\", \"Puebla\"], \" to \")",
        },
        Builtin::Lower => Signature {
            name,
            params: &["text: Text"],
            result: Kind(ValueType::Text),
            documentation: "Convert text to lowercase.",
            example: "\"Hello\"",
            tier,
            group: Group::Text,
            try_: "slug := lower(\"Oaxaca\")",
        },
        Builtin::Upper => Signature {
            name,
            params: &["text: Text"],
            result: Kind(ValueType::Text),
            documentation: "Convert text to uppercase.",
            example: "\"Hello\"",
            tier,
            group: Group::Text,
            try_: "code := upper(\"mxn\")",
        },
        Builtin::Replace => Signature {
            name,
            params: &["text: Text", "from: Text", "to: Text"],
            result: Kind(ValueType::Text),
            documentation: "Replace text occurrences.",
            example: "\"hello\", \"h\", \"j\"",
            tier,
            group: Group::Text,
            try_: "slug := replace(\"mexico city\", \" \", \"-\")",
        },
        Builtin::Sum => Signature {
            name,
            params: &["items: List or Table", "expression?: row calculation"],
            result: Words("Number, Money, Ratio, or Duration"),
            documentation: "Add compatible quantities from a list, skipping nulls, or a row expression over each table row, keeping units.",
            example: "groceries, quantity * price",
            tier,
            group: Group::Lists,
            try_: "prices := [$3, $4.50]\ntotal := sum(prices)",
        },
        Builtin::Countdown => Signature {
            name,
            params: &[
                "duration: Duration",
                "elapsed?: Duration",
                "started?: DateTime",
            ],
            result: Kind(ValueType::Countdown),
            documentation: "An idle countdown. Use Start timer to capture a timestamp; elapsed and started are persisted by timer controls.",
            example: "25m",
            tier,
            group: Group::Timers,
            try_: "focus := countdown(25m)\n\n- [ ] One focused session @timer(focus)",
        },
        Builtin::Stopwatch => Signature {
            name,
            params: &["elapsed?: Duration", "started?: DateTime"],
            result: Kind(ValueType::Stopwatch),
            documentation: "An idle stopwatch. Use Start, Pause, Resume, or Reset timer. Elapsed time includes time while the editor is closed.",
            example: "",
            tier,
            group: Group::Timers,
            try_: "work := stopwatch()\n\n- [ ] Investigate the flaky test @timer(work)",
        },
        Builtin::Maximize => Signature {
            name,
            params: &["objective: linear expression"],
            result: Kind(ValueType::Plan),
            documentation: "Declare a linear plan over the | constraint | expression | table below; undefined names become decisions.",
            example: "3 * bagels + 1.25 * doughnuts",
            tier,
            group: Group::Plans,
            try_: "400:flour_stock\n\nbakery := maximize($3 * bagels + $1.25 * doughnuts)\n| constraint | expression                                   |\n| ---------- | -------------------------------------------- |\n| flour      | 12 * bagels + 6.5 * doughnuts <= flour_stock |\n| milk       | bagels + 0.5 * doughnuts <= 200              |",
        },
        Builtin::Solve => Signature {
            name,
            params: &["constraint: expression with <=, >=, or =="],
            result: Words("Number, Money, or Duration"),
            documentation: "Goal seek: the definition's own name is the unknown, set to the boundary value that makes the constraint hold.",
            example: "total >= $500",
            tier,
            group: Group::Plans,
            try_: "$1,200:saved\n9:months_left\nsaved_by_june := monthly * months_left + saved\nmonthly := solve(saved_by_june >= $5,000)",
        },
        Builtin::Minimize => Signature {
            name,
            params: &["objective: linear expression"],
            result: Kind(ValueType::Plan),
            documentation: "Like maximize, but finds the smallest objective that satisfies every constraint in the table below.",
            example: "cost",
            tier,
            group: Group::Plans,
            try_: "plan := minimize(2 * crates)\n| constraint | expression   |\n| ---------- | ------------ |\n| demand     | crates >= 30 |",
        },
        Builtin::Today => Signature {
            name,
            params: &[],
            result: Kind(ValueType::Date),
            documentation: "The current local calendar date. Updates at midnight.",
            example: "",
            tier,
            group: Group::Dates,
            try_: "2026-11-20:departure\ndays_left := departure - today()",
        },
        Builtin::Now => Signature {
            name,
            params: &[],
            result: Kind(ValueType::DateTime),
            documentation: "The current timestamp. Sampled once per evaluation; live hints refresh every second.",
            example: "",
            tier,
            group: Group::Dates,
            try_: "stamp := now()\nin_an_hour := stamp + 1h",
        },
        Builtin::Rate => Signature {
            name,
            params: &["from: currency code", "to: currency code"],
            result: Kind(ValueType::Number),
            documentation: "The cached exchange rate between two currencies, e.g. rate(EUR, USD). Refresh with wtf refresh or the ⟳ lookups lens; hovers show the age.",
            example: "EUR, USD",
            tier,
            group: Group::Lookups,
            try_: "one_euro := rate(EUR, USD)",
        },
        Builtin::To => Signature {
            name,
            params: &["amount: Money", "currency: code"],
            result: Kind(ValueType::Money),
            documentation: "Convert money with the cached rate, e.g. to(hotel, USD); without one the note warns until wtf refresh.",
            example: "hotel, USD",
            tier,
            group: Group::Lookups,
            try_: "hotel := 700 MXN\nusd := to(hotel, USD)",
        },
        Builtin::Forecast => Signature {
            name,
            params: &["place: Text", "date: Date", "unit?: F or C"],
            result: Kind(ValueType::Forecast),
            documentation: "The cached forecast for a place and day, with .high, .low, .summary and .rain. Beyond 16 days, returns a seasonal outlook for up to about 7 months, labeled as an estimate. Seasonal .rain is the fraction of available ensemble runs with more than 0.1 mm of daily precipitation (including snow); it is unavailable with fewer than two valid runs. Itinerary days with a place get one automatically.",
            example: "\"Oaxaca\", 2026-11-20",
            tier,
            group: Group::Lookups,
            try_: "landing := forecast(\"Oaxaca\", 2026-11-20)",
        },
        Builtin::ForecastRange => Signature {
            name,
            params: &["place: Text", "start: Date", "end: Date", "unit?: F or C"],
            result: Kind(ValueType::List),
            documentation: "Cached daily forecasts in chronological order, including both dates. Project .high, .low, or .rain and pass the list to sparkline. One refresh requests the entire interval. Dates beyond 16 days use seasonal estimates; missing days remain lookup warnings until available. Limited to 4096 days.",
            example: "\"Oaxaca\", 2026-11-20, 2026-11-26, F",
            tier,
            group: Group::Lookups,
            try_: "week := forecast_range(\"Oaxaca\", 2026-11-20, 2026-11-26, F)\nHighs °F: [sparkline(week.high)]\nRain: [sparkline(week.rain, 0%, 100%)]",
        },
        Builtin::Quote => Signature {
            name,
            params: &["symbol: ticker code"],
            result: Kind(ValueType::Money),
            documentation: "The cached last price for a ticker, e.g. quote(NVDA); non-US tickers need a provider in .wtf/providers.json.",
            example: "NVDA",
            tier,
            group: Group::Lookups,
            try_: "12:shares\nholding := quote(NVDA) * shares",
        },
        Builtin::Date => Signature {
            name,
            params: &["value: Text, Date, or DateTime"],
            result: Words("Date or DateTime"),
            documentation: "Parse ISO or relative date text, or take a timestamp's calendar date in the request timezone.",
            example: "\"next Friday\"",
            tier,
            group: Group::Dates,
            try_: "friday := date(\"next Friday\")",
        },
        Builtin::Effort => Signature {
            name,
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Duration),
            documentation: "Sum estimates of unfinished leaf tasks beneath a named heading.",
            example: "checklist",
            tier,
            group: Group::Tasks,
            try_: "## Launch :launch\n\n- [ ] Record the demo @estimate(45m)\n- [ ] Update the changelog @estimate(20m)\n\nwork := effort(launch)",
        },
        Builtin::Total => Signature {
            name,
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Count),
            documentation: "Count all leaf tasks beneath a named heading.",
            example: "checklist",
            tier,
            group: Group::Tasks,
            try_: "## Launch :launch\n\n- [x] Draft the announcement\n- [ ] Ship it\n\nsteps := total(launch)",
        },
        Builtin::Completed => Signature {
            name,
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Count),
            documentation: "Count completed leaf tasks beneath a named heading.",
            example: "checklist",
            tier,
            group: Group::Tasks,
            try_: "## Launch :launch\n\n- [x] Draft the announcement\n- [ ] Ship it\n\ndone := completed(launch)",
        },
        Builtin::Remaining => Signature {
            name,
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Count),
            documentation: "Count unfinished leaf tasks beneath a named heading.",
            example: "checklist",
            tier,
            group: Group::Tasks,
            try_: "## Launch :launch\n\n- [x] Draft the announcement\n- [ ] Ship it\n\nleft := remaining(launch)",
        },
    }
}

/// Task and appointment attributes, which are written like calls but name no
/// built-in function.
const ATTRIBUTES: &[Signature] = &[
    Signature {
        name: "@timer",
        params: &["timer: Timer"],
        result: Words("task attribute"),
        documentation: "Associate a named timer with this task. Completion does not stop the timer.",
        example: "focus",
        tier: Tier::Note,
        group: Group::Tasks,
        try_: "focus := countdown(25m)\n\n- [ ] One focused session @timer(focus)",
    },
    Signature {
        name: "@due",
        params: &["date: Date or DateTime"],
        result: Words("task attribute"),
        documentation: "Deadline. Accepts a named date, an expression, or relative input such as tomorrow. Use Freeze relative date to capture it.",
        example: "tomorrow",
        tier: Tier::Note,
        group: Group::Tasks,
        try_: "2026-11-20:departure\n\n- [ ] Book the hotel @due(departure - 14d)",
    },
    Signature {
        name: "@scheduled",
        params: &["date: Date or DateTime"],
        result: Words("task attribute"),
        documentation: "Planned work date; separate from the deadline.",
        example: "tomorrow",
        tier: Tier::Note,
        group: Group::Tasks,
        try_: "- [ ] Book flights @scheduled(2026-10-01) @due(2026-10-10)",
    },
    Signature {
        name: "@at",
        params: &["time: Date or DateTime"],
        result: Words("appointment attribute"),
        documentation: "Appointment time. Include an explicit UTC offset for ambiguous local times.",
        example: "2026-09-18T14:00-04:00",
        tier: Tier::Note,
        group: Group::Tasks,
        try_: "- Dentist @at(2026-09-25T09:30-04:00)",
    },
    Signature {
        name: "@estimate",
        params: &["effort: Duration"],
        result: Words("task attribute"),
        documentation: "A nonnegative estimate. Examples: 30s, 20m, 2h.",
        example: "20m",
        tier: Tier::Note,
        group: Group::Tasks,
        try_: "- [ ] Update the changelog @estimate(20m)",
    },
    Signature {
        name: "@after",
        params: &["dependency: Boolean or Checklist", "more dependencies..."],
        result: Words("task attribute"),
        documentation: "Block this task until all dependencies are satisfied. Cycles are reported with source locations.",
        example: "task_name",
        tier: Tier::Note,
        group: Group::Tasks,
        try_: "- [x] Order parts :parts\n- [ ] Assemble @after(parts)",
    },
    Signature {
        name: "@every",
        params: &["interval: recurrence"],
        result: Words("task attribute"),
        documentation: "Repeat a leaf task: day, week, month, year, or a positive whole-day duration such as 2w.",
        example: "week",
        tier: Tier::Note,
        group: Group::Tasks,
        try_: "- [ ] Water the plants @every(week) @due(2026-09-20)",
    },
    Signature {
        name: "@tag",
        params: &["tag: name"],
        result: Words("task attribute"),
        documentation: "Tag a task for filtering. Multiple tags may be comma-separated.",
        example: "errands",
        tier: Tier::Note,
        group: Group::Tasks,
        try_: "- [ ] Renew passport @tag(errands)",
    },
];

const COUNT: usize = Builtin::ALL.len() + ATTRIBUTES.len();
/// The built-ins first, in `Builtin::ALL` order, then the attributes; both
/// signature help and completion read the table in this order.
const fn table() -> [Signature; COUNT] {
    let mut table = [describe(Builtin::Import); COUNT];
    let mut i = 0;
    while i < Builtin::ALL.len() {
        table[i] = describe(Builtin::ALL[i]);
        i += 1;
    }
    while i < COUNT {
        table[i] = ATTRIBUTES[i - Builtin::ALL.len()];
        i += 1;
    }
    table
}
const TABLE: [Signature; COUNT] = table();
pub(crate) static BUILTINS: &[Signature] = &TABLE;

pub fn signature(
    doc: &Document,
    path: &std::path::Path,
    position: Position,
) -> Option<SignatureHelp> {
    if inert(doc, position) {
        return None;
    }
    let line = doc.line(position.line as usize);
    let byte = byte_at(line, position.character)?;
    let (name, argument) = call_context(&line[..byte])?;
    let function = BUILTINS.iter().find(|f| f.name == name)?;
    // A note has no module-tier built-ins, so it is told nothing about them.
    if function.tier == Tier::Module && !eval::modules::is_module_path(path) {
        return None;
    }
    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: format!(
                "{}({}) → {}",
                function.name,
                function.params.join(", "),
                function.result.as_str()
            ),
            documentation: Some(Documentation::MarkupContent(markup(
                function.documentation.into(),
            ))),
            parameters: Some(
                function
                    .params
                    .iter()
                    .map(|p| ParameterInformation {
                        label: ParameterLabel::Simple((*p).into()),
                        documentation: None,
                    })
                    .collect(),
            ),
            active_parameter: None,
        }],
        active_signature: Some(0),
        active_parameter: (!function.params.is_empty())
            .then_some(argument.min(function.params.len().saturating_sub(1) as u32)),
    })
}
