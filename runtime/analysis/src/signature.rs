//! Signature help, over the one table of built-in calls and task attributes
//! that also feeds completion.
use crate::{hover::markup, locate::inert};
use lang::eval::engine::{Builtin, ValueType};
use lang::model::{Document, byte_at};
use lsp_types::*;

/// What a call answers with: one value kind wherever the answer has one, so
/// the table cannot invent a type name, and prose for the unions and for the
/// attributes that produce no value at all.
#[derive(Clone, Copy)]
pub enum Outcome {
    Type(ValueType),
    Words(&'static str),
}
impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Type(kind) => kind.as_str(),
            Self::Words(words) => words,
        }
    }
}

// A built-in's tier is declared with it in `syntax`, since the engine itself
// needs to know which built-ins module code alone may call.
use lang::eval::engine::Tier;

/// One built-in call or task attribute, as signature help and completion show
/// it: `documentation` explains it and `example` is what signature help fills
/// in. The table below is the only description of them the editor has.
#[derive(Clone, Copy)]
pub struct Signature {
    pub name: &'static str,
    pub params: &'static [&'static str],
    pub result: Outcome,
    pub documentation: &'static str,
    pub example: &'static str,
    pub tier: Tier,
}

/// One row per call: `Name(params) -> result, documentation, example;`. A
/// result that is an identifier names a value type, a string literal is prose.
macro_rules! signatures {
    (@outcome $kind:ident) => { Outcome::Type(ValueType::$kind) };
    (@outcome $words:literal) => { Outcome::Words($words) };
    (@row $name:expr, ($($param:literal),*), $result:tt, $documentation:expr, $example:literal, $tier:expr) => {
        Signature {
            name: $name,
            params: &[$($param),*],
            result: signatures!(@outcome $result),
            documentation: $documentation,
            example: $example,
            tier: $tier,
        }
    };
    (
        builtins { $($variant:ident($($param:literal),*) -> $result:tt, $documentation:expr, $example:literal;)* }
        attributes { $($attribute:literal($($attribute_param:literal),*) -> $attribute_result:tt, $attribute_documentation:expr, $attribute_example:literal;)* }
    ) => {
        /// Every built-in has exactly one description, and the compiler checks
        /// it: a new `Builtin` variant does not compile until this match
        /// answers for it. Its name and tier come from the `Builtin` itself.
        const fn describe(builtin: Builtin) -> Signature {
            match builtin {
                $(Builtin::$variant => signatures!(@row builtin.as_str(), ($($param),*), $result, $documentation, $example, builtin.tier()),)*
            }
        }

        /// Task and appointment attributes, which are written like calls but
        /// name no built-in function.
        const ATTRIBUTES: &[Signature] = &[
            $(signatures!(@row $attribute, ($($attribute_param),*), $attribute_result, $attribute_documentation, $attribute_example, Tier::Note),)*
        ];
    };
}

signatures! {
    builtins {
        Import("id: Text") -> Record,
            concat!(
                "Load a module by ID, or a note by path such as import(\"./values.",
                lang::common::note_extension!(),
                "\"); members keep their own source."
            ),
            "\"format\"";
        SolveLinear("model: Record") -> Record, "Solve a bounded linear model and return raw numeric values and status.", "model";
        Object("entries: List") -> Record, "Build a record from key/value pairs; duplicate keys are rejected.", "[{key: \"x\", value: 1}]";
        ParseDate("text: Text", "format: Text") -> "Date or Null", "Parse a calendar date with a strftime format; invalid input returns null.", "\"2026-09-18\", \"%F\"";
        ParseDatetime("text: Text", "format: Text", "offset: DateTime") -> "DateTime or Null", "Parse a local timestamp using a reference timestamp's offset.", "\"2026-09-18 09:30\", \"%F %H:%M\", now()";
        Entries("record: Record") -> List, "List key/value pairs in key order.", "{x: 1}";
        Number("value: Number, Money, Ratio, Duration or Count") -> Number, "Extract the numeric magnitude; durations use seconds.", "90m";
        Source("value: Any") -> Text, "Format a typed scalar as a round-trippable expression.", "now()";
        MakeDate("year: Number", "month: Number", "day: Number") -> "Date or Null", "Construct a calendar date; invalid dates return null.", "2026, 9, 18";
        Merge3("base: Text", "ours: Text", "theirs: Text") -> Record, "Merge two edits of a common base line by line: {clean, text}, with conflict markers when both changed the same lines.", "base, ours, theirs";
        UrlEncode("text: Text") -> Text, "Percent-encode text for a URL query or path segment, spaces as +.", "\"New York\"";
        DurationParts("duration: Duration") -> Record, "Split integer seconds into total hours, remaining minutes and seconds without rounding.", "90m";
        DateParts("date: Date or DateTime") -> Record, "Read year, month, day and weekday (Monday is zero).", "today()";
        AtTime("date: Date", "time: Duration", "offset: DateTime") -> DateTime, "Combine a date and time of day using the reference timestamp offset.", "today(), 9h, now()";
        ParseTime("text: Text", "format: Text") -> "Duration or Null", "Parse a time of day as seconds since midnight.", "\"09:30\", \"%H:%M\"";
        ParseDuration("text: Text") -> "Duration or Null", "Parse a written duration.", "\"2h\"";
        PadStart("text: Text", "width: Number", "fill: Text") -> Text, "Pad text to a character width with one character.", "\"3\", 2, \"0\"";
        PadEnd("text: Text", "width: Number", "fill: Text") -> Text, "Pad text on the right.", "\"x\", 3, \" \"";
        Slice("value: Text or List", "start: Number", "end: Number") -> "Text or List", "Take a half-open range; text indices count Unicode characters.", "\"hello\", 0, 2";
        Concat("lists: List...") -> List, "Concatenate lists.", "[1, 2], [3]";
        Trim("text: Text") -> Text, "Remove surrounding whitespace.", "\" hello \"";
        Type("value: Any") -> Text, "Get the runtime type name.", "42";
        Floor("number: Number") -> Number, "Round down to an integer.", "1.5";
        Round("number: Number") -> Number, "Round to the nearest integer.", "1.5";
        Repeat("text: Text", "count: Number") -> Text, "Repeat text a bounded number of times.", "\"█\", 3";
        FormatDate("date: Date or DateTime", "format: Text") -> Text, "Format a date or timestamp with strftime directives.", "today(), \"%Y-%m-%d\"";
        Error("message: Text") -> "Never", "Return an evaluation error.", "\"Missing data\"";
        If("condition: Boolean", "then: Value", "else: Value") -> "Value", "Evaluate only the selected branch; more condition, result pairs may come before the else.", "n < 0, \"negative\", n == 0, \"zero\", \"positive\"";
        Match("value: Value", "case: Value", "result: Value", "otherwise: Value") -> "Value", "Pick the result of the first case equal to the value, else the last argument; more case, result pairs may follow the first.", "state, \"open\", \"○\", \"done\", \"✓\", \"?\"";
        Let("names: Record", "body: Value") -> "Value", "Name values for the body; each name can use the ones before it.", "{x: 2, y: x * 3}, x + y";
        Coalesce("values: Value...") -> "Value", "Return the first non-null value.", "null, 1";
        Map("items: List", "function: Function") -> List, "Apply a pure function to every item.", "[1, 2], fn(x) => x * 2";
        Filter("items: List", "predicate: Function") -> List, "Keep items whose predicate returns true.", "[1, 2], fn(x) => x > 1";
        SortBy("items: List", "key: Function") -> List, "Stable ascending sort by a compatible scalar key; nulls come last.", "[3, 1], fn(x) => x";
        GroupBy("items: List", "key: Function") -> List, "Group by a scalar key into {key, rows} records, in first-seen order.", "[1, 2, 1], fn(x) => x";
        Eval("expression: Text") -> "Value", "Evaluate expression text in the current document's scope.", "\"price * 2\"";
        Fold("items: List", "initial: Value", "function: Function") -> "Value", "Combine items left to right with an accumulator.", "[1, 2], 0, fn(a, x) => a + x";
        Get("collection: Record or List", "key: Text or Number") -> "Value", "Read a field or index; return null when absent.", "{name: \"hello\"}, \"name\"";
        Length("value: List, Record, or Text") -> Count, "Count items, fields, or Unicode characters.", "\"hello\"";
        Text("value: Value") -> Text, "Format a value as text; null remains null.", "$25";
        Debug("value: Value") -> Text, "Inspect any value as compact JSON text in an inlay. Records, lists, and host objects expose their fields; money, dates, durations, and ratios keep their type and units.", "{rain: 35%, pack: true}";
        Sparkline("values: List", "min?: Number, Money, Ratio, or Duration", "max?: Number, Money, Ratio, or Duration") -> Text, "Draw one Unicode bar per numeric value, in list order. Null leaves a gap (·). The scale uses the data's minimum and maximum unless both bounds are supplied; values outside fixed bounds are clipped. Values and bounds must use matching units.", "[12, 18, 9, 24]";
        Contains("value: List or Text", "part: Value") -> Boolean, "Test membership or a text substring.", "\"hello\", \"ell\"";
        StartsWith("text: Text", "prefix: Text") -> Boolean, "Test a text prefix.", "\"hello\", \"he\"";
        EndsWith("text: Text", "suffix: Text") -> Boolean, "Test a text suffix.", "\"hello\", \"lo\"";
        Split("text: Text", "separator: Text") -> List, "Split text into pieces.", "\"a/b\", \"/\"";
        Join("items: List", "separator: Text") -> Text, "Join a list of text.", "[\"a\", \"b\"], \"/\"";
        Lower("text: Text") -> Text, "Convert text to lowercase.", "\"Hello\"";
        Upper("text: Text") -> Text, "Convert text to uppercase.", "\"Hello\"";
        Replace("text: Text", "from: Text", "to: Text") -> Text, "Replace text occurrences.", "\"hello\", \"h\", \"j\"";
        Sum("items: List or Table", "expression?: row calculation") -> "Number, Money, Ratio, or Duration", "Add compatible quantities from a list, skipping nulls, or a row expression over each table row, keeping units.", "groceries, quantity * price";
        Countdown("duration: Duration", "elapsed?: Duration", "started?: DateTime") -> Countdown, "An idle countdown. Use Start timer to capture a timestamp; elapsed and started are persisted by timer controls.", "25m";
        Stopwatch("elapsed?: Duration", "started?: DateTime") -> Stopwatch, "An idle stopwatch. Use Start, Pause, Resume, or Reset timer. Elapsed time includes time while the editor is closed.", "";
        Maximize("objective: linear expression") -> Plan, "Declare a linear plan over the | constraint | expression | table below; undefined names become decisions.", "3 * bagels + 1.25 * doughnuts";
        Solve("constraint: expression with <=, >=, or ==") -> "Number, Money, or Duration", "Goal seek: the definition's own name is the unknown, set to the boundary value that makes the constraint hold.", "total >= $500";
        Minimize("objective: linear expression") -> Plan, "Like maximize, but finds the smallest objective that satisfies every constraint in the table below.", "cost";
        Today() -> Date, "The current local calendar date. Updates at midnight.", "";
        Now() -> DateTime, "The current timestamp. Sampled once per evaluation; live hints refresh every second.", "";
        Rate("from: currency code", "to: currency code") -> Number, "The cached exchange rate between two currencies, e.g. rate(EUR, USD). Refresh with xmd refresh or the ⟳ lookups lens; hovers show the age.", "EUR, USD";
        To("amount: Money", "currency: code") -> Money, "Convert money with the cached rate, e.g. to(hotel, USD); without one the note warns until xmd refresh.", "hotel, USD";
        Forecast("place: Text", "date: Date", "unit?: F or C") -> Forecast, "The cached forecast for a place and day, with .high, .low, .summary and .rain. Beyond 16 days, returns a seasonal outlook for up to about 7 months, labeled as an estimate. Seasonal .rain is the fraction of available ensemble runs with more than 0.1 mm of daily precipitation (including snow); it is unavailable with fewer than two valid runs. Itinerary days with a place get one automatically.", "\"Oaxaca\", 2026-11-20";
        ForecastRange("place: Text", "start: Date", "end: Date", "unit?: F or C") -> List, "Cached daily forecasts in chronological order, including both dates. Project .high, .low, or .rain and pass the list to sparkline. One refresh requests the entire interval. Dates beyond 16 days use seasonal estimates; missing days remain lookup warnings until available. Limited to 4096 days.", "\"Oaxaca\", 2026-11-20, 2026-11-26, F";
        Quote("symbol: ticker code") -> Money, "The cached last price for a ticker, e.g. quote(NVDA); non-US tickers need a provider in .xmd/providers.json.", "NVDA";
        Date("value: Text, Date, or DateTime") -> "Date or DateTime", "Parse ISO or relative date text, or take a timestamp's calendar date in the request timezone.", "\"next Friday\"";
        Effort("checklist: Checklist") -> Duration, "Sum estimates of unfinished leaf tasks beneath a named heading.", "checklist";
        Total("checklist: Checklist") -> Count, "Count all leaf tasks beneath a named heading.", "checklist";
        Completed("checklist: Checklist") -> Count, "Count completed leaf tasks beneath a named heading.", "checklist";
        Remaining("checklist: Checklist") -> Count, "Count unfinished leaf tasks beneath a named heading.", "checklist";
    }
    attributes {
        "@timer"("timer: Timer") -> "task attribute", "Associate a named timer with this task. Completion does not stop the timer.", "focus";
        "@due"("date: Date or DateTime") -> "task attribute", "Deadline. Accepts a named date, an expression, or relative input such as tomorrow. Use Freeze relative date to capture it.", "tomorrow";
        "@scheduled"("date: Date or DateTime") -> "task attribute", "Planned work date; separate from the deadline.", "tomorrow";
        "@at"("time: Date or DateTime") -> "appointment attribute", "Appointment time. Include an explicit UTC offset for ambiguous local times.", "2026-09-18T14:00-04:00";
        "@estimate"("effort: Duration") -> "task attribute", "A nonnegative estimate. Examples: 30s, 20m, 2h.", "20m";
        "@after"("dependency: Boolean or Checklist", "more dependencies...") -> "task attribute", "Block this task until all dependencies are satisfied. Cycles are reported with source locations.", "task_name";
        "@every"("interval: recurrence") -> "task attribute", "Repeat a leaf task: day, week, month, year, or a positive whole-day duration such as 2w.", "week";
        "@tag"("tag: name") -> "task attribute", "Tag a task for filtering. Multiple tags may be comma-separated.", "errands";
    }
}

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
pub static BUILTINS: &[Signature] = &TABLE;

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
    if function.tier == Tier::Module && !lang::eval::modules::is_module_path(path) {
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

/// Tolerates incomplete calls, quoted strings and nested parentheses.
pub fn call_context(prefix: &str) -> Option<(&str, u32)> {
    let mut stack: Vec<(&str, u32)> = vec![];
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in prefix.char_indices() {
        if quoted {
            if c == '"' && !escaped {
                quoted = false;
            }
            escaped = c == '\\' && !escaped;
            continue;
        }
        match c {
            '"' => quoted = true,
            '(' => {
                let before = prefix[..i].trim_end();
                let name = before
                    .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '@')
                    .next()
                    .unwrap_or("");
                stack.push((name, 0));
            }
            ')' => {
                stack.pop();
            }
            ',' if !(i > 0
                && prefix.as_bytes()[i - 1].is_ascii_digit()
                && prefix.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                if let Some((_, argument)) = stack.last_mut() {
                    *argument += 1;
                }
            }
            _ => {}
        }
    }
    stack.into_iter().rev().find(|(name, _)| !name.is_empty())
}
