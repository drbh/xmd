//! Signature help, over the one table of built-in calls and task attributes
//! that also feeds completion.
use crate::{
    completion::call_context,
    document::{Document, byte_at},
    engine::ValueType,
    hover::markup,
    intelligence::inert,
};
use Outcome::{Kind, Words};
use lsp_types::*;

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
/// One built-in call or task attribute, as signature help and completion show
/// it. The table below is the only description of them the editor has.
pub(crate) struct Builtin {
    pub(crate) name: &'static str,
    pub(crate) params: &'static [&'static str],
    pub(crate) result: Outcome,
    pub(crate) documentation: &'static str,
    pub(crate) example: &'static str,
}
pub(crate) fn is_builtin_function(name: &str) -> bool {
    crate::engine::is_builtin_function(name)
}

pub(crate) const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "import",
        params: &["id: Text"],
        result: Kind(ValueType::Record),
        documentation: "Load a module by ID, or a note with an explicit path such as import(\"./values.wtf\"). Note names are file-local; imported members retain their original source.",
        example: "\"format\"",
    },
    Builtin {
        name: "solve_linear",
        params: &["model: Record"],
        result: Kind(ValueType::Record),
        documentation: "Solve a bounded linear model and return raw numeric values and status.",
        example: "model",
    },
    Builtin {
        name: "object",
        params: &["entries: List"],
        result: Kind(ValueType::Record),
        documentation: "Build a record from key/value pairs; duplicate keys are rejected.",
        example: "[{key: \"x\", value: 1}]",
    },
    Builtin {
        name: "parse_date",
        params: &["text: Text", "format: Text"],
        result: Words("Date or Null"),
        documentation: "Parse a calendar date with a strftime format; invalid input returns null.",
        example: "\"2026-09-18\", \"%F\"",
    },
    Builtin {
        name: "parse_datetime",
        params: &["text: Text", "format: Text", "offset: DateTime"],
        result: Words("DateTime or Null"),
        documentation: "Parse a local timestamp using a reference timestamp's offset.",
        example: "\"2026-09-18 09:30\", \"%F %H:%M\", now()",
    },
    Builtin {
        name: "entries",
        params: &["record: Record"],
        result: Kind(ValueType::List),
        documentation: "List key/value pairs in key order.",
        example: "{x: 1}",
    },
    Builtin {
        name: "number",
        params: &["value: Number, Money, Ratio, Duration or Count"],
        result: Kind(ValueType::Number),
        documentation: "Extract the numeric magnitude; durations use seconds.",
        example: "90m",
    },
    Builtin {
        name: "source",
        params: &["value: Any"],
        result: Kind(ValueType::Text),
        documentation: "Format a typed scalar as a round-trippable expression.",
        example: "now()",
    },
    Builtin {
        name: "make_date",
        params: &["year: Number", "month: Number", "day: Number"],
        result: Words("Date or Null"),
        documentation: "Construct a calendar date; invalid dates return null.",
        example: "2026, 9, 18",
    },
    Builtin {
        name: "duration_parts",
        params: &["duration: Duration"],
        result: Kind(ValueType::Record),
        documentation: "Split integer seconds into total hours, remaining minutes and seconds without rounding.",
        example: "90m",
    },
    Builtin {
        name: "date_parts",
        params: &["date: Date or DateTime"],
        result: Kind(ValueType::Record),
        documentation: "Read year, month, day and weekday (Monday is zero).",
        example: "today()",
    },
    Builtin {
        name: "at_time",
        params: &["date: Date", "time: Duration", "offset: DateTime"],
        result: Kind(ValueType::DateTime),
        documentation: "Combine a date and time of day using the reference timestamp offset.",
        example: "today(), 9h, now()",
    },
    Builtin {
        name: "parse_time",
        params: &["text: Text", "format: Text"],
        result: Words("Duration or Null"),
        documentation: "Parse a time of day as seconds since midnight.",
        example: "\"09:30\", \"%H:%M\"",
    },
    Builtin {
        name: "parse_duration",
        params: &["text: Text"],
        result: Words("Duration or Null"),
        documentation: "Parse a written duration.",
        example: "\"2h\"",
    },
    Builtin {
        name: "pad_start",
        params: &["text: Text", "width: Number", "fill: Text"],
        result: Kind(ValueType::Text),
        documentation: "Pad text to a character width with one character.",
        example: "\"3\", 2, \"0\"",
    },
    Builtin {
        name: "pad_end",
        params: &["text: Text", "width: Number", "fill: Text"],
        result: Kind(ValueType::Text),
        documentation: "Pad text on the right.",
        example: "\"x\", 3, \" \"",
    },
    Builtin {
        name: "slice",
        params: &["value: Text or List", "start: Number", "end: Number"],
        result: Words("Text or List"),
        documentation: "Take a half-open range; text indices count Unicode characters.",
        example: "\"hello\", 0, 2",
    },
    Builtin {
        name: "concat",
        params: &["lists: List..."],
        result: Kind(ValueType::List),
        documentation: "Concatenate lists.",
        example: "[1, 2], [3]",
    },
    Builtin {
        name: "trim",
        params: &["text: Text"],
        result: Kind(ValueType::Text),
        documentation: "Remove surrounding whitespace.",
        example: "\" hello \"",
    },
    Builtin {
        name: "type",
        params: &["value: Any"],
        result: Kind(ValueType::Text),
        documentation: "Get the runtime type name.",
        example: "42",
    },
    Builtin {
        name: "floor",
        params: &["number: Number"],
        result: Kind(ValueType::Number),
        documentation: "Round down to an integer.",
        example: "1.5",
    },
    Builtin {
        name: "round",
        params: &["number: Number"],
        result: Kind(ValueType::Number),
        documentation: "Round to the nearest integer.",
        example: "1.5",
    },
    Builtin {
        name: "repeat",
        params: &["text: Text", "count: Number"],
        result: Kind(ValueType::Text),
        documentation: "Repeat text a bounded number of times.",
        example: "\"█\", 3",
    },
    Builtin {
        name: "format_date",
        params: &["date: Date or DateTime", "format: Text"],
        result: Kind(ValueType::Text),
        documentation: "Format a date or timestamp with strftime directives.",
        example: "today(), \"%Y-%m-%d\"",
    },
    Builtin {
        name: "error",
        params: &["message: Text"],
        result: Words("Never"),
        documentation: "Return an evaluation error.",
        example: "\"Missing data\"",
    },
    Builtin {
        name: "if",
        params: &["condition: Boolean", "then: Value", "else: Value"],
        result: Words("Value"),
        documentation: "Evaluate only the selected branch.",
        example: "true, 1, 0",
    },
    Builtin {
        name: "coalesce",
        params: &["values: Value..."],
        result: Words("Value"),
        documentation: "Return the first non-null value.",
        example: "null, 1",
    },
    Builtin {
        name: "map",
        params: &["items: List", "function: Function"],
        result: Kind(ValueType::List),
        documentation: "Apply a pure function to every item.",
        example: "[1, 2], fn(x) => x * 2",
    },
    Builtin {
        name: "filter",
        params: &["items: List", "predicate: Function"],
        result: Kind(ValueType::List),
        documentation: "Keep items whose predicate returns true.",
        example: "[1, 2], fn(x) => x > 1",
    },
    Builtin {
        name: "sort_by",
        params: &["items: List", "key: Function"],
        result: Kind(ValueType::List),
        documentation: "Stable ascending sort by a compatible scalar key; nulls come last.",
        example: "[3, 1], fn(x) => x",
    },
    Builtin {
        name: "group_by",
        params: &["items: List", "key: Function"],
        result: Kind(ValueType::List),
        documentation: "Group by a scalar key into {key, rows} records, in first-seen order.",
        example: "[1, 2, 1], fn(x) => x",
    },
    Builtin {
        name: "eval",
        params: &["expression: Text"],
        result: Words("Value"),
        documentation: "Evaluate expression text in the current document's scope.",
        example: "\"price * 2\"",
    },
    Builtin {
        name: "fold",
        params: &["items: List", "initial: Value", "function: Function"],
        result: Words("Value"),
        documentation: "Combine items left to right with an accumulator.",
        example: "[1, 2], 0, fn(a, x) => a + x",
    },
    Builtin {
        name: "get",
        params: &["collection: Record or List", "key: Text or Number"],
        result: Words("Value"),
        documentation: "Read a field or index; return null when absent.",
        example: "{name: \"hello\"}, \"name\"",
    },
    Builtin {
        name: "length",
        params: &["value: List, Record, or Text"],
        result: Kind(ValueType::Count),
        documentation: "Count items, fields, or Unicode characters.",
        example: "\"hello\"",
    },
    Builtin {
        name: "text",
        params: &["value: Value"],
        result: Kind(ValueType::Text),
        documentation: "Format a value as text; null remains null.",
        example: "$25",
    },
    Builtin {
        name: "contains",
        params: &["value: List or Text", "part: Value"],
        result: Kind(ValueType::Boolean),
        documentation: "Test membership or a text substring.",
        example: "\"hello\", \"ell\"",
    },
    Builtin {
        name: "starts_with",
        params: &["text: Text", "prefix: Text"],
        result: Kind(ValueType::Boolean),
        documentation: "Test a text prefix.",
        example: "\"hello\", \"he\"",
    },
    Builtin {
        name: "ends_with",
        params: &["text: Text", "suffix: Text"],
        result: Kind(ValueType::Boolean),
        documentation: "Test a text suffix.",
        example: "\"hello\", \"lo\"",
    },
    Builtin {
        name: "split",
        params: &["text: Text", "separator: Text"],
        result: Kind(ValueType::List),
        documentation: "Split text into pieces.",
        example: "\"a/b\", \"/\"",
    },
    Builtin {
        name: "join",
        params: &["items: List", "separator: Text"],
        result: Kind(ValueType::Text),
        documentation: "Join a list of text.",
        example: "[\"a\", \"b\"], \"/\"",
    },
    Builtin {
        name: "lower",
        params: &["text: Text"],
        result: Kind(ValueType::Text),
        documentation: "Convert text to lowercase.",
        example: "\"Hello\"",
    },
    Builtin {
        name: "upper",
        params: &["text: Text"],
        result: Kind(ValueType::Text),
        documentation: "Convert text to uppercase.",
        example: "\"Hello\"",
    },
    Builtin {
        name: "replace",
        params: &["text: Text", "from: Text", "to: Text"],
        result: Kind(ValueType::Text),
        documentation: "Replace text occurrences.",
        example: "\"hello\", \"h\", \"j\"",
    },
    Builtin {
        name: "sum",
        params: &["items: List or Table", "expression?: row calculation"],
        result: Words("Number, Money, Ratio, or Duration"),
        documentation: "Sum a list of compatible quantities, skipping nulls, or evaluate a row expression for each table row and add the results. Units are preserved.",
        example: "groceries, quantity * price",
    },
    Builtin {
        name: "countdown",
        params: &[
            "duration: Duration",
            "elapsed?: Duration",
            "started?: DateTime",
        ],
        result: Kind(ValueType::Countdown),
        documentation: "An idle countdown. Use Start timer to capture a timestamp; elapsed and started are persisted by timer controls.",
        example: "25m",
    },
    Builtin {
        name: "stopwatch",
        params: &["elapsed?: Duration", "started?: DateTime"],
        result: Kind(ValueType::Stopwatch),
        documentation: "An idle stopwatch. Use Start, Pause, Resume, or Reset timer. Elapsed time includes time while the editor is closed.",
        example: "",
    },
    Builtin {
        name: "maximize",
        params: &["objective: linear expression"],
        result: Kind(ValueType::Plan),
        documentation: "Declare a linear plan: the next lines hold a | constraint | expression | table. Names no note defines become decision variables (at least zero); everything else is a constant. Example: maximize(3 * bagels + 1.25 * doughnuts).",
        example: "3 * bagels + 1.25 * doughnuts",
    },
    Builtin {
        name: "solve",
        params: &["constraint: expression with <=, >=, or =="],
        result: Words("Number, Money, or Duration"),
        documentation: "Goal seek: the definition's own name is the unknown, and the answer is the boundary value that makes the constraint hold through any chain of calculations. Example: [monthly] := solve(saved_by_june >= $5,000).",
        example: "total >= $500",
    },
    Builtin {
        name: "minimize",
        params: &["objective: linear expression"],
        result: Kind(ValueType::Plan),
        documentation: "Like maximize, but finds the smallest objective that satisfies every constraint in the table below.",
        example: "cost",
    },
    Builtin {
        name: "today",
        params: &[],
        result: Kind(ValueType::Date),
        documentation: "The current local calendar date. Updates at midnight.",
        example: "",
    },
    Builtin {
        name: "now",
        params: &[],
        result: Kind(ValueType::DateTime),
        documentation: "The current timestamp. Sampled once per evaluation; live hints refresh every second.",
        example: "",
    },
    Builtin {
        name: "rate",
        params: &["from: currency code", "to: currency code"],
        result: Kind(ValueType::Number),
        documentation: "The cached exchange rate between two currencies, e.g. rate(EUR, USD). Refresh with wtf refresh or the ⟳ lookups lens; hovers show the age.",
        example: "EUR, USD",
    },
    Builtin {
        name: "to",
        params: &["amount: Money", "currency: code"],
        result: Kind(ValueType::Money),
        documentation: "Convert money using the cached rate, e.g. to(hotel, USD). Money in different currencies never adds up silently.",
        example: "hotel, USD",
    },
    Builtin {
        name: "forecast",
        params: &["place: Text", "date: Date", "unit?: F or C"],
        result: Kind(ValueType::Forecast),
        documentation: "The cached forecast for a place and day, with .high, .low, .summary and .rain. Itinerary days with a place get one automatically.",
        example: "\"Oaxaca\", 2026-11-20",
    },
    Builtin {
        name: "quote",
        params: &["symbol: ticker code"],
        result: Kind(ValueType::Money),
        documentation: "The cached last price for a ticker, e.g. quote(NVDA). The built-in source covers US tickers; set a quote provider in .wtf/providers.json for others.",
        example: "NVDA",
    },
    Builtin {
        name: "date",
        params: &["value: Text, Date, or DateTime"],
        result: Words("Date or DateTime"),
        documentation: "Parse ISO or relative date text, or take a timestamp's calendar date in the request timezone.",
        example: "\"next Friday\"",
    },
    Builtin {
        name: "effort",
        params: &["checklist: Checklist"],
        result: Kind(ValueType::Duration),
        documentation: "Sum estimates of unfinished leaf tasks beneath a named heading.",
        example: "checklist",
    },
    Builtin {
        name: "total",
        params: &["checklist: Checklist"],
        result: Kind(ValueType::Count),
        documentation: "Count all leaf tasks beneath a named heading.",
        example: "checklist",
    },
    Builtin {
        name: "completed",
        params: &["checklist: Checklist"],
        result: Kind(ValueType::Count),
        documentation: "Count completed leaf tasks beneath a named heading.",
        example: "checklist",
    },
    Builtin {
        name: "remaining",
        params: &["checklist: Checklist"],
        result: Kind(ValueType::Count),
        documentation: "Count unfinished leaf tasks beneath a named heading.",
        example: "checklist",
    },
    Builtin {
        name: "@timer",
        params: &["timer: Timer"],
        result: Words("task attribute"),
        documentation: "Associate a named timer with this task. Completion does not stop the timer.",
        example: "focus",
    },
    Builtin {
        name: "@due",
        params: &["date: Date or DateTime"],
        result: Words("task attribute"),
        documentation: "Deadline. Accepts a named date, an expression, or relative input such as tomorrow. Use Freeze relative date to capture it.",
        example: "tomorrow",
    },
    Builtin {
        name: "@scheduled",
        params: &["date: Date or DateTime"],
        result: Words("task attribute"),
        documentation: "Planned work date; separate from the deadline.",
        example: "tomorrow",
    },
    Builtin {
        name: "@at",
        params: &["time: Date or DateTime"],
        result: Words("appointment attribute"),
        documentation: "Appointment time. Include an explicit UTC offset for ambiguous local times.",
        example: "2026-09-18T14:00-04:00",
    },
    Builtin {
        name: "@estimate",
        params: &["effort: Duration"],
        result: Words("task attribute"),
        documentation: "A nonnegative estimate. Examples: 30s, 20m, 2h.",
        example: "20m",
    },
    Builtin {
        name: "@after",
        params: &["dependency: Boolean or Checklist", "more dependencies..."],
        result: Words("task attribute"),
        documentation: "Block this task until all dependencies are satisfied. Cycles are reported with source locations.",
        example: "task_name",
    },
    Builtin {
        name: "@every",
        params: &["interval: recurrence"],
        result: Words("task attribute"),
        documentation: "Repeat a leaf task: day, week, month, year, or a positive whole-day duration such as 2w.",
        example: "week",
    },
    Builtin {
        name: "@tag",
        params: &["tag: name"],
        result: Words("task attribute"),
        documentation: "Tag a task for filtering. Multiple tags may be comma-separated.",
        example: "errands",
    },
];

pub fn signature(doc: &Document, position: Position) -> Option<SignatureHelp> {
    if inert(doc, position) {
        return None;
    }
    let line = doc.line(position.line as usize);
    let byte = byte_at(line, position.character)?;
    let (name, argument) = call_context(&line[..byte])?;
    let function = BUILTINS.iter().find(|f| f.name == name)?;
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
