//! Signature help, over the one table of built-in calls and task attributes
//! that also feeds completion.
use crate::{
    completion::call_context,
    document::{Document, byte_at},
    engine::{Builtin, ValueType},
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
#[derive(Clone, Copy)]
pub(crate) struct Signature {
    pub(crate) name: &'static str,
    pub(crate) params: &'static [&'static str],
    pub(crate) result: Outcome,
    pub(crate) documentation: &'static str,
    pub(crate) example: &'static str,
}
pub(crate) fn is_builtin_function(name: &str) -> bool {
    crate::engine::is_builtin_function(name)
}

/// Every built-in has exactly one description, and the compiler checks it: a
/// new `Builtin` variant does not compile until this match answers for it.
const fn describe(builtin: Builtin) -> Signature {
    let name = builtin.as_str();
    match builtin {
        Builtin::Import => Signature {
            name,
            params: &["id: Text"],
            result: Kind(ValueType::Record),
            documentation: "Load a module by ID, or a note with an explicit path such as import(\"./values.wtf\"). Note names are file-local; imported members retain their original source.",
            example: "\"format\"",
        },
        Builtin::SolveLinear => Signature {
            name,
            params: &["model: Record"],
            result: Kind(ValueType::Record),
            documentation: "Solve a bounded linear model and return raw numeric values and status.",
            example: "model",
        },
        Builtin::Object => Signature {
            name,
            params: &["entries: List"],
            result: Kind(ValueType::Record),
            documentation: "Build a record from key/value pairs; duplicate keys are rejected.",
            example: "[{key: \"x\", value: 1}]",
        },
        Builtin::ParseDate => Signature {
            name,
            params: &["text: Text", "format: Text"],
            result: Words("Date or Null"),
            documentation: "Parse a calendar date with a strftime format; invalid input returns null.",
            example: "\"2026-09-18\", \"%F\"",
        },
        Builtin::ParseDatetime => Signature {
            name,
            params: &["text: Text", "format: Text", "offset: DateTime"],
            result: Words("DateTime or Null"),
            documentation: "Parse a local timestamp using a reference timestamp's offset.",
            example: "\"2026-09-18 09:30\", \"%F %H:%M\", now()",
        },
        Builtin::Entries => Signature {
            name,
            params: &["record: Record"],
            result: Kind(ValueType::List),
            documentation: "List key/value pairs in key order.",
            example: "{x: 1}",
        },
        Builtin::Number => Signature {
            name,
            params: &["value: Number, Money, Ratio, Duration or Count"],
            result: Kind(ValueType::Number),
            documentation: "Extract the numeric magnitude; durations use seconds.",
            example: "90m",
        },
        Builtin::Source => Signature {
            name,
            params: &["value: Any"],
            result: Kind(ValueType::Text),
            documentation: "Format a typed scalar as a round-trippable expression.",
            example: "now()",
        },
        Builtin::MakeDate => Signature {
            name,
            params: &["year: Number", "month: Number", "day: Number"],
            result: Words("Date or Null"),
            documentation: "Construct a calendar date; invalid dates return null.",
            example: "2026, 9, 18",
        },
        Builtin::DurationParts => Signature {
            name,
            params: &["duration: Duration"],
            result: Kind(ValueType::Record),
            documentation: "Split integer seconds into total hours, remaining minutes and seconds without rounding.",
            example: "90m",
        },
        Builtin::DateParts => Signature {
            name,
            params: &["date: Date or DateTime"],
            result: Kind(ValueType::Record),
            documentation: "Read year, month, day and weekday (Monday is zero).",
            example: "today()",
        },
        Builtin::AtTime => Signature {
            name,
            params: &["date: Date", "time: Duration", "offset: DateTime"],
            result: Kind(ValueType::DateTime),
            documentation: "Combine a date and time of day using the reference timestamp offset.",
            example: "today(), 9h, now()",
        },
        Builtin::ParseTime => Signature {
            name,
            params: &["text: Text", "format: Text"],
            result: Words("Duration or Null"),
            documentation: "Parse a time of day as seconds since midnight.",
            example: "\"09:30\", \"%H:%M\"",
        },
        Builtin::ParseDuration => Signature {
            name,
            params: &["text: Text"],
            result: Words("Duration or Null"),
            documentation: "Parse a written duration.",
            example: "\"2h\"",
        },
        Builtin::PadStart => Signature {
            name,
            params: &["text: Text", "width: Number", "fill: Text"],
            result: Kind(ValueType::Text),
            documentation: "Pad text to a character width with one character.",
            example: "\"3\", 2, \"0\"",
        },
        Builtin::PadEnd => Signature {
            name,
            params: &["text: Text", "width: Number", "fill: Text"],
            result: Kind(ValueType::Text),
            documentation: "Pad text on the right.",
            example: "\"x\", 3, \" \"",
        },
        Builtin::Slice => Signature {
            name,
            params: &["value: Text or List", "start: Number", "end: Number"],
            result: Words("Text or List"),
            documentation: "Take a half-open range; text indices count Unicode characters.",
            example: "\"hello\", 0, 2",
        },
        Builtin::Concat => Signature {
            name,
            params: &["lists: List..."],
            result: Kind(ValueType::List),
            documentation: "Concatenate lists.",
            example: "[1, 2], [3]",
        },
        Builtin::Trim => Signature {
            name,
            params: &["text: Text"],
            result: Kind(ValueType::Text),
            documentation: "Remove surrounding whitespace.",
            example: "\" hello \"",
        },
        Builtin::Type => Signature {
            name,
            params: &["value: Any"],
            result: Kind(ValueType::Text),
            documentation: "Get the runtime type name.",
            example: "42",
        },
        Builtin::Floor => Signature {
            name,
            params: &["number: Number"],
            result: Kind(ValueType::Number),
            documentation: "Round down to an integer.",
            example: "1.5",
        },
        Builtin::Round => Signature {
            name,
            params: &["number: Number"],
            result: Kind(ValueType::Number),
            documentation: "Round to the nearest integer.",
            example: "1.5",
        },
        Builtin::Repeat => Signature {
            name,
            params: &["text: Text", "count: Number"],
            result: Kind(ValueType::Text),
            documentation: "Repeat text a bounded number of times.",
            example: "\"█\", 3",
        },
        Builtin::FormatDate => Signature {
            name,
            params: &["date: Date or DateTime", "format: Text"],
            result: Kind(ValueType::Text),
            documentation: "Format a date or timestamp with strftime directives.",
            example: "today(), \"%Y-%m-%d\"",
        },
        Builtin::Error => Signature {
            name,
            params: &["message: Text"],
            result: Words("Never"),
            documentation: "Return an evaluation error.",
            example: "\"Missing data\"",
        },
        Builtin::If => Signature {
            name,
            params: &["condition: Boolean", "then: Value", "else: Value"],
            result: Words("Value"),
            documentation: "Evaluate only the selected branch.",
            example: "true, 1, 0",
        },
        Builtin::Coalesce => Signature {
            name,
            params: &["values: Value..."],
            result: Words("Value"),
            documentation: "Return the first non-null value.",
            example: "null, 1",
        },
        Builtin::Map => Signature {
            name,
            params: &["items: List", "function: Function"],
            result: Kind(ValueType::List),
            documentation: "Apply a pure function to every item.",
            example: "[1, 2], fn(x) => x * 2",
        },
        Builtin::Filter => Signature {
            name,
            params: &["items: List", "predicate: Function"],
            result: Kind(ValueType::List),
            documentation: "Keep items whose predicate returns true.",
            example: "[1, 2], fn(x) => x > 1",
        },
        Builtin::SortBy => Signature {
            name,
            params: &["items: List", "key: Function"],
            result: Kind(ValueType::List),
            documentation: "Stable ascending sort by a compatible scalar key; nulls come last.",
            example: "[3, 1], fn(x) => x",
        },
        Builtin::GroupBy => Signature {
            name,
            params: &["items: List", "key: Function"],
            result: Kind(ValueType::List),
            documentation: "Group by a scalar key into {key, rows} records, in first-seen order.",
            example: "[1, 2, 1], fn(x) => x",
        },
        Builtin::Eval => Signature {
            name,
            params: &["expression: Text"],
            result: Words("Value"),
            documentation: "Evaluate expression text in the current document's scope.",
            example: "\"price * 2\"",
        },
        Builtin::Fold => Signature {
            name,
            params: &["items: List", "initial: Value", "function: Function"],
            result: Words("Value"),
            documentation: "Combine items left to right with an accumulator.",
            example: "[1, 2], 0, fn(a, x) => a + x",
        },
        Builtin::Get => Signature {
            name,
            params: &["collection: Record or List", "key: Text or Number"],
            result: Words("Value"),
            documentation: "Read a field or index; return null when absent.",
            example: "{name: \"hello\"}, \"name\"",
        },
        Builtin::Length => Signature {
            name,
            params: &["value: List, Record, or Text"],
            result: Kind(ValueType::Count),
            documentation: "Count items, fields, or Unicode characters.",
            example: "\"hello\"",
        },
        Builtin::Text => Signature {
            name,
            params: &["value: Value"],
            result: Kind(ValueType::Text),
            documentation: "Format a value as text; null remains null.",
            example: "$25",
        },
        Builtin::Contains => Signature {
            name,
            params: &["value: List or Text", "part: Value"],
            result: Kind(ValueType::Boolean),
            documentation: "Test membership or a text substring.",
            example: "\"hello\", \"ell\"",
        },
        Builtin::StartsWith => Signature {
            name,
            params: &["text: Text", "prefix: Text"],
            result: Kind(ValueType::Boolean),
            documentation: "Test a text prefix.",
            example: "\"hello\", \"he\"",
        },
        Builtin::EndsWith => Signature {
            name,
            params: &["text: Text", "suffix: Text"],
            result: Kind(ValueType::Boolean),
            documentation: "Test a text suffix.",
            example: "\"hello\", \"lo\"",
        },
        Builtin::Split => Signature {
            name,
            params: &["text: Text", "separator: Text"],
            result: Kind(ValueType::List),
            documentation: "Split text into pieces.",
            example: "\"a/b\", \"/\"",
        },
        Builtin::Join => Signature {
            name,
            params: &["items: List", "separator: Text"],
            result: Kind(ValueType::Text),
            documentation: "Join a list of text.",
            example: "[\"a\", \"b\"], \"/\"",
        },
        Builtin::Lower => Signature {
            name,
            params: &["text: Text"],
            result: Kind(ValueType::Text),
            documentation: "Convert text to lowercase.",
            example: "\"Hello\"",
        },
        Builtin::Upper => Signature {
            name,
            params: &["text: Text"],
            result: Kind(ValueType::Text),
            documentation: "Convert text to uppercase.",
            example: "\"Hello\"",
        },
        Builtin::Replace => Signature {
            name,
            params: &["text: Text", "from: Text", "to: Text"],
            result: Kind(ValueType::Text),
            documentation: "Replace text occurrences.",
            example: "\"hello\", \"h\", \"j\"",
        },
        Builtin::Sum => Signature {
            name,
            params: &["items: List or Table", "expression?: row calculation"],
            result: Words("Number, Money, Ratio, or Duration"),
            documentation: "Sum a list of compatible quantities, skipping nulls, or evaluate a row expression for each table row and add the results. Units are preserved.",
            example: "groceries, quantity * price",
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
        },
        Builtin::Stopwatch => Signature {
            name,
            params: &["elapsed?: Duration", "started?: DateTime"],
            result: Kind(ValueType::Stopwatch),
            documentation: "An idle stopwatch. Use Start, Pause, Resume, or Reset timer. Elapsed time includes time while the editor is closed.",
            example: "",
        },
        Builtin::Maximize => Signature {
            name,
            params: &["objective: linear expression"],
            result: Kind(ValueType::Plan),
            documentation: "Declare a linear plan: the next lines hold a | constraint | expression | table. Names no note defines become decision variables (at least zero); everything else is a constant. Example: maximize(3 * bagels + 1.25 * doughnuts).",
            example: "3 * bagels + 1.25 * doughnuts",
        },
        Builtin::Solve => Signature {
            name,
            params: &["constraint: expression with <=, >=, or =="],
            result: Words("Number, Money, or Duration"),
            documentation: "Goal seek: the definition's own name is the unknown, and the answer is the boundary value that makes the constraint hold through any chain of calculations. Example: [monthly] := solve(saved_by_june >= $5,000).",
            example: "total >= $500",
        },
        Builtin::Minimize => Signature {
            name,
            params: &["objective: linear expression"],
            result: Kind(ValueType::Plan),
            documentation: "Like maximize, but finds the smallest objective that satisfies every constraint in the table below.",
            example: "cost",
        },
        Builtin::Today => Signature {
            name,
            params: &[],
            result: Kind(ValueType::Date),
            documentation: "The current local calendar date. Updates at midnight.",
            example: "",
        },
        Builtin::Now => Signature {
            name,
            params: &[],
            result: Kind(ValueType::DateTime),
            documentation: "The current timestamp. Sampled once per evaluation; live hints refresh every second.",
            example: "",
        },
        Builtin::Rate => Signature {
            name,
            params: &["from: currency code", "to: currency code"],
            result: Kind(ValueType::Number),
            documentation: "The cached exchange rate between two currencies, e.g. rate(EUR, USD). Refresh with wtf refresh or the ⟳ lookups lens; hovers show the age.",
            example: "EUR, USD",
        },
        Builtin::To => Signature {
            name,
            params: &["amount: Money", "currency: code"],
            result: Kind(ValueType::Money),
            documentation: "Convert money using the cached rate, e.g. to(hotel, USD). Money in different currencies never adds up silently.",
            example: "hotel, USD",
        },
        Builtin::Forecast => Signature {
            name,
            params: &["place: Text", "date: Date", "unit?: F or C"],
            result: Kind(ValueType::Forecast),
            documentation: "The cached forecast for a place and day, with .high, .low, .summary and .rain. Itinerary days with a place get one automatically.",
            example: "\"Oaxaca\", 2026-11-20",
        },
        Builtin::Quote => Signature {
            name,
            params: &["symbol: ticker code"],
            result: Kind(ValueType::Money),
            documentation: "The cached last price for a ticker, e.g. quote(NVDA). The built-in source covers US tickers; set a quote provider in .wtf/providers.json for others.",
            example: "NVDA",
        },
        Builtin::Date => Signature {
            name,
            params: &["value: Text, Date, or DateTime"],
            result: Words("Date or DateTime"),
            documentation: "Parse ISO or relative date text, or take a timestamp's calendar date in the request timezone.",
            example: "\"next Friday\"",
        },
        Builtin::Effort => Signature {
            name,
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Duration),
            documentation: "Sum estimates of unfinished leaf tasks beneath a named heading.",
            example: "checklist",
        },
        Builtin::Total => Signature {
            name,
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Count),
            documentation: "Count all leaf tasks beneath a named heading.",
            example: "checklist",
        },
        Builtin::Completed => Signature {
            name,
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Count),
            documentation: "Count completed leaf tasks beneath a named heading.",
            example: "checklist",
        },
        Builtin::Remaining => Signature {
            name,
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Count),
            documentation: "Count unfinished leaf tasks beneath a named heading.",
            example: "checklist",
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
    },
    Signature {
        name: "@due",
        params: &["date: Date or DateTime"],
        result: Words("task attribute"),
        documentation: "Deadline. Accepts a named date, an expression, or relative input such as tomorrow. Use Freeze relative date to capture it.",
        example: "tomorrow",
    },
    Signature {
        name: "@scheduled",
        params: &["date: Date or DateTime"],
        result: Words("task attribute"),
        documentation: "Planned work date; separate from the deadline.",
        example: "tomorrow",
    },
    Signature {
        name: "@at",
        params: &["time: Date or DateTime"],
        result: Words("appointment attribute"),
        documentation: "Appointment time. Include an explicit UTC offset for ambiguous local times.",
        example: "2026-09-18T14:00-04:00",
    },
    Signature {
        name: "@estimate",
        params: &["effort: Duration"],
        result: Words("task attribute"),
        documentation: "A nonnegative estimate. Examples: 30s, 20m, 2h.",
        example: "20m",
    },
    Signature {
        name: "@after",
        params: &["dependency: Boolean or Checklist", "more dependencies..."],
        result: Words("task attribute"),
        documentation: "Block this task until all dependencies are satisfied. Cycles are reported with source locations.",
        example: "task_name",
    },
    Signature {
        name: "@every",
        params: &["interval: recurrence"],
        result: Words("task attribute"),
        documentation: "Repeat a leaf task: day, week, month, year, or a positive whole-day duration such as 2w.",
        example: "week",
    },
    Signature {
        name: "@tag",
        params: &["tag: name"],
        result: Words("task attribute"),
        documentation: "Tag a task for filtering. Multiple tags may be comma-separated.",
        example: "errands",
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
