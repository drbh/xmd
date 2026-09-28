//! Signature help, over the one table of built-in calls and task attributes
//! that also feeds completion.
use crate::{language::hover::markup, locate::inert};
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

// A built-in's tier is declared with it in `syntax`, since the engine itself
// needs to know which built-ins module code alone may call.
pub(crate) use eval::engine::Tier;

/// One built-in call or task attribute, as signature help and completion show
/// it: `documentation` explains it and `example` is what signature help fills
/// in. The table below is the only description of them the editor has.
#[derive(Clone, Copy)]
pub(crate) struct Signature {
    pub(crate) name: &'static str,
    pub(crate) params: &'static [&'static str],
    pub(crate) result: Outcome,
    pub(crate) documentation: &'static str,
    pub(crate) example: &'static str,
    pub(crate) tier: Tier,
}
pub(crate) fn is_builtin_function(name: &str) -> bool {
    eval::engine::is_builtin_function(name)
}

/// What a built-in's row in `describe` says; its name and tier come from the
/// `Builtin` itself, so the table never restates them.
struct Doc {
    params: &'static [&'static str],
    result: Outcome,
    documentation: &'static str,
    example: &'static str,
}
impl Doc {
    const fn signature(self, builtin: Builtin) -> Signature {
        Signature {
            name: builtin.as_str(),
            params: self.params,
            result: self.result,
            documentation: self.documentation,
            example: self.example,
            tier: builtin.tier(),
        }
    }
}

/// Every built-in has exactly one description, and the compiler checks it: a
/// new `Builtin` variant does not compile until this match answers for it.
const fn describe(builtin: Builtin) -> Signature {
    let doc = match builtin {
        Builtin::Import => Doc {
            params: &["id: Text"],
            result: Kind(ValueType::Record),
            documentation: "Load a module by ID, or a note by path such as import(\"./values.wtf\"); members keep their own source.",
            example: "\"format\"",
        },
        Builtin::SolveLinear => Doc {
            params: &["model: Record"],
            result: Kind(ValueType::Record),
            documentation: "Solve a bounded linear model and return raw numeric values and status.",
            example: "model",
        },
        Builtin::Object => Doc {
            params: &["entries: List"],
            result: Kind(ValueType::Record),
            documentation: "Build a record from key/value pairs; duplicate keys are rejected.",
            example: "[{key: \"x\", value: 1}]",
        },
        Builtin::ParseDate => Doc {
            params: &["text: Text", "format: Text"],
            result: Words("Date or Null"),
            documentation: "Parse a calendar date with a strftime format; invalid input returns null.",
            example: "\"2026-09-18\", \"%F\"",
        },
        Builtin::ParseDatetime => Doc {
            params: &["text: Text", "format: Text", "offset: DateTime"],
            result: Words("DateTime or Null"),
            documentation: "Parse a local timestamp using a reference timestamp's offset.",
            example: "\"2026-09-18 09:30\", \"%F %H:%M\", now()",
        },
        Builtin::Entries => Doc {
            params: &["record: Record"],
            result: Kind(ValueType::List),
            documentation: "List key/value pairs in key order.",
            example: "{x: 1}",
        },
        Builtin::Number => Doc {
            params: &["value: Number, Money, Ratio, Duration or Count"],
            result: Kind(ValueType::Number),
            documentation: "Extract the numeric magnitude; durations use seconds.",
            example: "90m",
        },
        Builtin::Source => Doc {
            params: &["value: Any"],
            result: Kind(ValueType::Text),
            documentation: "Format a typed scalar as a round-trippable expression.",
            example: "now()",
        },
        Builtin::MakeDate => Doc {
            params: &["year: Number", "month: Number", "day: Number"],
            result: Words("Date or Null"),
            documentation: "Construct a calendar date; invalid dates return null.",
            example: "2026, 9, 18",
        },
        Builtin::DurationParts => Doc {
            params: &["duration: Duration"],
            result: Kind(ValueType::Record),
            documentation: "Split integer seconds into total hours, remaining minutes and seconds without rounding.",
            example: "90m",
        },
        Builtin::DateParts => Doc {
            params: &["date: Date or DateTime"],
            result: Kind(ValueType::Record),
            documentation: "Read year, month, day and weekday (Monday is zero).",
            example: "today()",
        },
        Builtin::AtTime => Doc {
            params: &["date: Date", "time: Duration", "offset: DateTime"],
            result: Kind(ValueType::DateTime),
            documentation: "Combine a date and time of day using the reference timestamp offset.",
            example: "today(), 9h, now()",
        },
        Builtin::ParseTime => Doc {
            params: &["text: Text", "format: Text"],
            result: Words("Duration or Null"),
            documentation: "Parse a time of day as seconds since midnight.",
            example: "\"09:30\", \"%H:%M\"",
        },
        Builtin::ParseDuration => Doc {
            params: &["text: Text"],
            result: Words("Duration or Null"),
            documentation: "Parse a written duration.",
            example: "\"2h\"",
        },
        Builtin::PadStart => Doc {
            params: &["text: Text", "width: Number", "fill: Text"],
            result: Kind(ValueType::Text),
            documentation: "Pad text to a character width with one character.",
            example: "\"3\", 2, \"0\"",
        },
        Builtin::PadEnd => Doc {
            params: &["text: Text", "width: Number", "fill: Text"],
            result: Kind(ValueType::Text),
            documentation: "Pad text on the right.",
            example: "\"x\", 3, \" \"",
        },
        Builtin::Slice => Doc {
            params: &["value: Text or List", "start: Number", "end: Number"],
            result: Words("Text or List"),
            documentation: "Take a half-open range; text indices count Unicode characters.",
            example: "\"hello\", 0, 2",
        },
        Builtin::Concat => Doc {
            params: &["lists: List..."],
            result: Kind(ValueType::List),
            documentation: "Concatenate lists.",
            example: "[1, 2], [3]",
        },
        Builtin::Trim => Doc {
            params: &["text: Text"],
            result: Kind(ValueType::Text),
            documentation: "Remove surrounding whitespace.",
            example: "\" hello \"",
        },
        Builtin::Type => Doc {
            params: &["value: Any"],
            result: Kind(ValueType::Text),
            documentation: "Get the runtime type name.",
            example: "42",
        },
        Builtin::Floor => Doc {
            params: &["number: Number"],
            result: Kind(ValueType::Number),
            documentation: "Round down to an integer.",
            example: "1.5",
        },
        Builtin::Round => Doc {
            params: &["number: Number"],
            result: Kind(ValueType::Number),
            documentation: "Round to the nearest integer.",
            example: "1.5",
        },
        Builtin::Repeat => Doc {
            params: &["text: Text", "count: Number"],
            result: Kind(ValueType::Text),
            documentation: "Repeat text a bounded number of times.",
            example: "\"█\", 3",
        },
        Builtin::FormatDate => Doc {
            params: &["date: Date or DateTime", "format: Text"],
            result: Kind(ValueType::Text),
            documentation: "Format a date or timestamp with strftime directives.",
            example: "today(), \"%Y-%m-%d\"",
        },
        Builtin::Error => Doc {
            params: &["message: Text"],
            result: Words("Never"),
            documentation: "Return an evaluation error.",
            example: "\"Missing data\"",
        },
        Builtin::If => Doc {
            params: &["condition: Boolean", "then: Value", "else: Value"],
            result: Words("Value"),
            documentation: "Evaluate only the selected branch.",
            example: "true, 1, 0",
        },
        Builtin::Coalesce => Doc {
            params: &["values: Value..."],
            result: Words("Value"),
            documentation: "Return the first non-null value.",
            example: "null, 1",
        },
        Builtin::Map => Doc {
            params: &["items: List", "function: Function"],
            result: Kind(ValueType::List),
            documentation: "Apply a pure function to every item.",
            example: "[1, 2], fn(x) => x * 2",
        },
        Builtin::Filter => Doc {
            params: &["items: List", "predicate: Function"],
            result: Kind(ValueType::List),
            documentation: "Keep items whose predicate returns true.",
            example: "[1, 2], fn(x) => x > 1",
        },
        Builtin::SortBy => Doc {
            params: &["items: List", "key: Function"],
            result: Kind(ValueType::List),
            documentation: "Stable ascending sort by a compatible scalar key; nulls come last.",
            example: "[3, 1], fn(x) => x",
        },
        Builtin::GroupBy => Doc {
            params: &["items: List", "key: Function"],
            result: Kind(ValueType::List),
            documentation: "Group by a scalar key into {key, rows} records, in first-seen order.",
            example: "[1, 2, 1], fn(x) => x",
        },
        Builtin::Eval => Doc {
            params: &["expression: Text"],
            result: Words("Value"),
            documentation: "Evaluate expression text in the current document's scope.",
            example: "\"price * 2\"",
        },
        Builtin::Fold => Doc {
            params: &["items: List", "initial: Value", "function: Function"],
            result: Words("Value"),
            documentation: "Combine items left to right with an accumulator.",
            example: "[1, 2], 0, fn(a, x) => a + x",
        },
        Builtin::Get => Doc {
            params: &["collection: Record or List", "key: Text or Number"],
            result: Words("Value"),
            documentation: "Read a field or index; return null when absent.",
            example: "{name: \"hello\"}, \"name\"",
        },
        Builtin::Length => Doc {
            params: &["value: List, Record, or Text"],
            result: Kind(ValueType::Count),
            documentation: "Count items, fields, or Unicode characters.",
            example: "\"hello\"",
        },
        Builtin::Text => Doc {
            params: &["value: Value"],
            result: Kind(ValueType::Text),
            documentation: "Format a value as text; null remains null.",
            example: "$25",
        },
        Builtin::Debug => Doc {
            params: &["value: Value"],
            result: Kind(ValueType::Text),
            documentation: "Inspect any value as compact JSON text in an inlay. Records, lists, and host objects expose their fields; money, dates, durations, and ratios keep their type and units.",
            example: "{rain: 35%, pack: true}",
        },
        Builtin::Sparkline => Doc {
            params: &[
                "values: List",
                "min?: Number, Money, Ratio, or Duration",
                "max?: Number, Money, Ratio, or Duration",
            ],
            result: Kind(ValueType::Text),
            documentation: "Draw one Unicode bar per numeric value, in list order. Null leaves a gap (·). The scale uses the data's minimum and maximum unless both bounds are supplied; values outside fixed bounds are clipped. Values and bounds must use matching units.",
            example: "[12, 18, 9, 24]",
        },
        Builtin::Contains => Doc {
            params: &["value: List or Text", "part: Value"],
            result: Kind(ValueType::Boolean),
            documentation: "Test membership or a text substring.",
            example: "\"hello\", \"ell\"",
        },
        Builtin::StartsWith => Doc {
            params: &["text: Text", "prefix: Text"],
            result: Kind(ValueType::Boolean),
            documentation: "Test a text prefix.",
            example: "\"hello\", \"he\"",
        },
        Builtin::EndsWith => Doc {
            params: &["text: Text", "suffix: Text"],
            result: Kind(ValueType::Boolean),
            documentation: "Test a text suffix.",
            example: "\"hello\", \"lo\"",
        },
        Builtin::Split => Doc {
            params: &["text: Text", "separator: Text"],
            result: Kind(ValueType::List),
            documentation: "Split text into pieces.",
            example: "\"a/b\", \"/\"",
        },
        Builtin::Join => Doc {
            params: &["items: List", "separator: Text"],
            result: Kind(ValueType::Text),
            documentation: "Join a list of text.",
            example: "[\"a\", \"b\"], \"/\"",
        },
        Builtin::Lower => Doc {
            params: &["text: Text"],
            result: Kind(ValueType::Text),
            documentation: "Convert text to lowercase.",
            example: "\"Hello\"",
        },
        Builtin::Upper => Doc {
            params: &["text: Text"],
            result: Kind(ValueType::Text),
            documentation: "Convert text to uppercase.",
            example: "\"Hello\"",
        },
        Builtin::Replace => Doc {
            params: &["text: Text", "from: Text", "to: Text"],
            result: Kind(ValueType::Text),
            documentation: "Replace text occurrences.",
            example: "\"hello\", \"h\", \"j\"",
        },
        Builtin::Sum => Doc {
            params: &["items: List or Table", "expression?: row calculation"],
            result: Words("Number, Money, Ratio, or Duration"),
            documentation: "Add compatible quantities from a list, skipping nulls, or a row expression over each table row, keeping units.",
            example: "groceries, quantity * price",
        },
        Builtin::Countdown => Doc {
            params: &[
                "duration: Duration",
                "elapsed?: Duration",
                "started?: DateTime",
            ],
            result: Kind(ValueType::Countdown),
            documentation: "An idle countdown. Use Start timer to capture a timestamp; elapsed and started are persisted by timer controls.",
            example: "25m",
        },
        Builtin::Stopwatch => Doc {
            params: &["elapsed?: Duration", "started?: DateTime"],
            result: Kind(ValueType::Stopwatch),
            documentation: "An idle stopwatch. Use Start, Pause, Resume, or Reset timer. Elapsed time includes time while the editor is closed.",
            example: "",
        },
        Builtin::Maximize => Doc {
            params: &["objective: linear expression"],
            result: Kind(ValueType::Plan),
            documentation: "Declare a linear plan over the | constraint | expression | table below; undefined names become decisions.",
            example: "3 * bagels + 1.25 * doughnuts",
        },
        Builtin::Solve => Doc {
            params: &["constraint: expression with <=, >=, or =="],
            result: Words("Number, Money, or Duration"),
            documentation: "Goal seek: the definition's own name is the unknown, set to the boundary value that makes the constraint hold.",
            example: "total >= $500",
        },
        Builtin::Minimize => Doc {
            params: &["objective: linear expression"],
            result: Kind(ValueType::Plan),
            documentation: "Like maximize, but finds the smallest objective that satisfies every constraint in the table below.",
            example: "cost",
        },
        Builtin::Today => Doc {
            params: &[],
            result: Kind(ValueType::Date),
            documentation: "The current local calendar date. Updates at midnight.",
            example: "",
        },
        Builtin::Now => Doc {
            params: &[],
            result: Kind(ValueType::DateTime),
            documentation: "The current timestamp. Sampled once per evaluation; live hints refresh every second.",
            example: "",
        },
        Builtin::Rate => Doc {
            params: &["from: currency code", "to: currency code"],
            result: Kind(ValueType::Number),
            documentation: "The cached exchange rate between two currencies, e.g. rate(EUR, USD). Refresh with wtf refresh or the ⟳ lookups lens; hovers show the age.",
            example: "EUR, USD",
        },
        Builtin::To => Doc {
            params: &["amount: Money", "currency: code"],
            result: Kind(ValueType::Money),
            documentation: "Convert money with the cached rate, e.g. to(hotel, USD); without one the note warns until wtf refresh.",
            example: "hotel, USD",
        },
        Builtin::Forecast => Doc {
            params: &["place: Text", "date: Date", "unit?: F or C"],
            result: Kind(ValueType::Forecast),
            documentation: "The cached forecast for a place and day, with .high, .low, .summary and .rain. Beyond 16 days, returns a seasonal outlook for up to about 7 months, labeled as an estimate. Seasonal .rain is the fraction of available ensemble runs with more than 0.1 mm of daily precipitation (including snow); it is unavailable with fewer than two valid runs. Itinerary days with a place get one automatically.",
            example: "\"Oaxaca\", 2026-11-20",
        },
        Builtin::ForecastRange => Doc {
            params: &["place: Text", "start: Date", "end: Date", "unit?: F or C"],
            result: Kind(ValueType::List),
            documentation: "Cached daily forecasts in chronological order, including both dates. Project .high, .low, or .rain and pass the list to sparkline. One refresh requests the entire interval. Dates beyond 16 days use seasonal estimates; missing days remain lookup warnings until available. Limited to 4096 days.",
            example: "\"Oaxaca\", 2026-11-20, 2026-11-26, F",
        },
        Builtin::Quote => Doc {
            params: &["symbol: ticker code"],
            result: Kind(ValueType::Money),
            documentation: "The cached last price for a ticker, e.g. quote(NVDA); non-US tickers need a provider in .wtf/providers.json.",
            example: "NVDA",
        },
        Builtin::Date => Doc {
            params: &["value: Text, Date, or DateTime"],
            result: Words("Date or DateTime"),
            documentation: "Parse ISO or relative date text, or take a timestamp's calendar date in the request timezone.",
            example: "\"next Friday\"",
        },
        Builtin::Effort => Doc {
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Duration),
            documentation: "Sum estimates of unfinished leaf tasks beneath a named heading.",
            example: "checklist",
        },
        Builtin::Total => Doc {
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Count),
            documentation: "Count all leaf tasks beneath a named heading.",
            example: "checklist",
        },
        Builtin::Completed => Doc {
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Count),
            documentation: "Count completed leaf tasks beneath a named heading.",
            example: "checklist",
        },
        Builtin::Remaining => Doc {
            params: &["checklist: Checklist"],
            result: Kind(ValueType::Count),
            documentation: "Count unfinished leaf tasks beneath a named heading.",
            example: "checklist",
        },
    };
    doc.signature(builtin)
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
    },
    Signature {
        name: "@due",
        params: &["date: Date or DateTime"],
        result: Words("task attribute"),
        documentation: "Deadline. Accepts a named date, an expression, or relative input such as tomorrow. Use Freeze relative date to capture it.",
        example: "tomorrow",
        tier: Tier::Note,
    },
    Signature {
        name: "@scheduled",
        params: &["date: Date or DateTime"],
        result: Words("task attribute"),
        documentation: "Planned work date; separate from the deadline.",
        example: "tomorrow",
        tier: Tier::Note,
    },
    Signature {
        name: "@at",
        params: &["time: Date or DateTime"],
        result: Words("appointment attribute"),
        documentation: "Appointment time. Include an explicit UTC offset for ambiguous local times.",
        example: "2026-09-18T14:00-04:00",
        tier: Tier::Note,
    },
    Signature {
        name: "@estimate",
        params: &["effort: Duration"],
        result: Words("task attribute"),
        documentation: "A nonnegative estimate. Examples: 30s, 20m, 2h.",
        example: "20m",
        tier: Tier::Note,
    },
    Signature {
        name: "@after",
        params: &["dependency: Boolean or Checklist", "more dependencies..."],
        result: Words("task attribute"),
        documentation: "Block this task until all dependencies are satisfied. Cycles are reported with source locations.",
        example: "task_name",
        tier: Tier::Note,
    },
    Signature {
        name: "@every",
        params: &["interval: recurrence"],
        result: Words("task attribute"),
        documentation: "Repeat a leaf task: day, week, month, year, or a positive whole-day duration such as 2w.",
        example: "week",
        tier: Tier::Note,
    },
    Signature {
        name: "@tag",
        params: &["tag: name"],
        result: Words("task attribute"),
        documentation: "Tag a task for filtering. Multiple tags may be comma-separated.",
        example: "errands",
        tier: Tier::Note,
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

/// Tolerates incomplete calls, quoted strings and nested parentheses.
pub(crate) fn call_context(prefix: &str) -> Option<(String, u32)> {
    let mut stack: Vec<(String, u32)> = vec![];
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
                stack.push((name.into(), 0));
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
