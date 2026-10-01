//! Signature help, over the one table of built-in calls that also feeds
//! completion, and the prelude's functions, which describe themselves in
//! their `.xmd` source. The attributes and forms modules declare describe
//! themselves, and a note knows them ([`Document::declarations`],
//! [`Document::forms_declared`]).
use crate::{hover::markup, locate::inert};
use lang::document::byte_at;
use lang::eval::Workspace;
use lang::eval::engine::{Builtin, ValueType};
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

/// One built-in call, as signature help and completion show it:
/// `documentation` explains it and `example` is what signature help fills
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
    ) => {
        /// Every built-in has exactly one description, and the compiler checks
        /// it: a new `Builtin` variant does not compile until this match
        /// answers for it. Its name and tier come from the `Builtin` itself.
        const fn describe(builtin: Builtin) -> Signature {
            match builtin {
                $(Builtin::$variant => signatures!(@row builtin.as_str(), ($($param),*), $result, $documentation, $example, builtin.tier()),)*
            }
        }
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
        Pending("message: Text") -> "Never", "Return an evaluation error that says the data is not available yet, such as a lookup nothing has fetched: hosts report it as a warning rather than a mistake in the note.", "\"No cached score; run xmd refresh\"";
        If("condition: Boolean", "then: Value", "else: Value") -> "Value", "Evaluate only the selected branch; more condition, result pairs may come before the else.", "n < 0, \"negative\", n == 0, \"zero\", \"positive\"";
        Match("value: Value", "case: Value", "result: Value", "otherwise: Value") -> "Value", "Pick the result of the first case equal to the value, else the last argument; more case, result pairs may follow the first.", "state, \"open\", \"○\", \"done\", \"✓\", \"?\"";
        Let("names: Record", "body: Value") -> "Value", "Name values for the body; each name can use the ones before it.", "{x: 2, y: x * 3}, x + y";
        Coalesce("values: Value...") -> "Value", "Return the first non-null value.", "null, 1";
        Map("items: List", "function: Function") -> List, "Apply a pure function to every item.", "[1, 2], fn(x) => x * 2";
        Filter("items: List", "predicate: Function") -> List, "Keep items whose predicate returns true.", "[1, 2], fn(x) => x > 1";
        SortBy("items: List", "key: Function, desc(key), or a list of them") -> List, "Stable sort by one key, or by several in order; desc(key) sorts that key descending. Nulls come last either way.", "tasks, [desc(.due), .title]";
        Desc("key: Function") -> Record, "Sort by a key descending, as a sort_by key.", ".due";
        GroupBy("items: List", "key: Function") -> List, "Group by a scalar key into {key, rows} records, in first-seen order.", "[1, 2, 1], fn(x) => x";
        Eval("expression: Text") -> "Value", "Evaluate expression text in the current document's scope.", "\"price * 2\"";
        Fold("items: List", "initial: Value", "function: Function") -> "Value", "Combine items left to right with an accumulator.", "[1, 2], 0, fn(a, x) => a + x";
        Get("collection: Record or List", "key: Text or Number") -> "Value", "Read a field or index; return null when absent.", "{name: \"hello\"}, \"name\"";
        Length("value: List, Record, or Text") -> Count, "Count items, fields, or Unicode characters.", "\"hello\"";
        Text("value: Value") -> Text, "Format a value as text; null remains null.", "$25";
        Debug("value: Value") -> Text, "Inspect any value as compact JSON text in an inlay. Records, lists, and host objects expose their fields; money, dates, durations, and ratios keep their type and units.", "{rain: 35%, pack: true}";
        Contains("value: List or Text", "part: Value") -> Boolean, "Test membership or a text substring.", "\"hello\", \"ell\"";
        StartsWith("text: Text", "prefix: Text") -> Boolean, "Test a text prefix.", "\"hello\", \"he\"";
        EndsWith("text: Text", "suffix: Text") -> Boolean, "Test a text suffix.", "\"hello\", \"lo\"";
        Split("text: Text", "separator: Text") -> List, "Split text into pieces.", "\"a/b\", \"/\"";
        Join("items: List", "separator: Text") -> Text, "Join a list of text.", "[\"a\", \"b\"], \"/\"";
        Lower("text: Text") -> Text, "Convert text to lowercase.", "\"Hello\"";
        Upper("text: Text") -> Text, "Convert text to uppercase.", "\"Hello\"";
        Replace("text: Text", "from: Text", "to: Text") -> Text, "Replace text occurrences.", "\"hello\", \"h\", \"j\"";
        Quantize("values: List", "levels: Number", "low: Number or Null", "high: Number or Null") -> List, "Each value's level from 0 to levels - 1, in equal steps between low and high (the values' own extremes when both are null) and clipped to them; null stays null, and equal bounds put every value on the middle level. Reads magnitudes as number does, so units are the caller's to check.", "[12, 18, 9, 24], 8, null, null";
        NextOccurrence("rule: Text", "anchor: Date", "after: Date") -> Record, "`{date, error}`: the first date after `after` that a recurrence counted from `anchor` falls on, or null with why there is none. A recurrence is `day`, `week`, `month` or `year` (or `daily`, `weekly`, `monthly`, `yearly`), or a positive whole-day duration such as `2w`. Months and years keep the anchor's day, or the month's last day when it has none (an anchor on the 31st comes back on the 30th, then the 31st).", "\"month\", 2026-01-31, today()";
        ToJson("value: Text, Number, Boolean, Null, List or Record") -> Text, "The value as compact JSON, record keys sorted. Anything else, a date included, is an error: write it as text first.", "{title: \"Rent\", due: \"2026-10-01\"}";
        EndPosition("text: Text") -> Record, "Where `text` ends as an editor counts: `{line, character}`, its line count and the UTF-16 length of its last line, or the next line's start when it ends in a line break. Where an edit appends to a note.", "ctx.document.text";
        DisplayWidth("text: Text") -> Count, "How many columns `text` takes in a terminal or a monospace editor: most characters take one, wide ones such as CJK and most emoji take two, and combining marks and zero-width characters none. It aligns text where padding to `length` would not.", "\"名前\"";
        MatchPattern("text: Text", "pattern: Text") -> "Record or Null", "The first match of a regular expression, or null: `{text, start, end, groups}`, offsets counting Unicode characters as `slice` does. `groups` has every named group `(?<name>...)` as `{text, start, end}`, or null when it took no part. Matching takes time linear in the text; a pattern is limited to 4096 bytes and compiled once.", "\"Ada: 42\", \"(?<name>\\\\w+): (?<n>\\\\d+)\"";
        Cached("kind: Text", "key: List of one-field records", "label?: Text") -> "Record or Null", "The workspace's cached answer for a lookup as {value, fetched_at, source}, or null before anything has fetched it. Every key read, cached or not, is one xmd refresh and the ⟳ lookups lens fetch, through the provider for its kind; hovers name it by its label and show its age. The key's parts are in the order the cache spells them: [{from: \"EUR\"}, {to: \"USD\"}] is rate:EUR:USD.", "\"rate\", [{from: \"EUR\"}, {to: \"USD\"}], \"rate EUR→USD\"";
        MakeMoney("amount: Number", "currency: Text") -> "Money or Null", "An amount of money in a currency code, or null when the code is not three uppercase letters.", "12.5, \"EUR\"";
        MakeRatio("fraction: Number") -> Ratio, "A number as a ratio: 0.4 is 40%.", "0.4";
        Tagged("kind: Text", "fields: Record", "display: Text", "hover?: Markdown") -> "the kind", "A record a note sees as a kind of its own, named by the module: a capitalized name of letters, digits and underscores that no built-in kind has. It reads its fields, names the kind in type, hovers and errors, shows the display text wherever it is shown, adds the hover to a symbol's hover, and is the plain record in queries and JSON. A record whose origin field is null learns the definition whose whole expression is the call that built it: {document, name, line, text, range, function, arguments}.", "\"Reading\", {celsius: 21}, \"21°C\"";
        Clocked("value: Value", "ticking: Function") -> "Value", "The value, which keeps depending on the clock it read only while ticking(value) is true: once ticking says no, the value no longer moves with the clock it read, so nothing refreshes it.", "state, fn(s) => s.running";
        Sum("items: List or Table", "expression?: row calculation") -> "Number, Money, Ratio, or Duration", "Add compatible quantities from a list, skipping nulls, or a row expression over each table row, keeping units.", "groceries, quantity * price";
        Today() -> Date, "The current local calendar date. Updates at midnight.", "";
        Now() -> DateTime, "The current timestamp. Sampled once per evaluation; live hints refresh every second.", "";
        Date("value: Text, Date, or DateTime") -> "Date or DateTime", "Parse ISO or relative date text, or take a timestamp's calendar date in the request timezone.", "\"next Friday\"";
    }
}

/// Every built-in, in `Builtin::ALL` order: what signature help and
/// completion read.
pub static BUILTINS: &[Signature] = &{
    let mut table = [describe(Builtin::Import); Builtin::ALL.len()];
    let mut i = 0;
    while i < Builtin::ALL.len() {
        table[i] = describe(Builtin::ALL[i]);
        i += 1;
    }
    table
};

pub fn signature(
    ws: &Workspace,
    path: &std::path::Path,
    position: Position,
) -> Option<SignatureHelp> {
    let doc = ws.documents().get(path)?;
    if inert(doc, position) {
        return None;
    }
    let line = doc.line(position.line as usize);
    let byte = byte_at(line, position.character)?;
    let (name, argument) = call_context(&line[..byte])?;
    let (label, params, documentation) = match BUILTINS.iter().find(|f| f.name == name) {
        // A note has no module-tier built-ins, so it is told nothing about them.
        Some(function)
            if function.tier == Tier::Module && !lang::eval::modules::is_module_path(path) =>
        {
            return None;
        }
        Some(function) => (
            format!(
                "{}({}) → {}",
                function.name,
                function.params.join(", "),
                function.result.as_str()
            ),
            function.params.iter().map(|p| p.to_string()).collect(),
            function.documentation.to_string(),
        ),
        // A form a module declares describes itself.
        None if let Some(form) = doc.declared_form(name) => (
            format!("{name}({}) → {}", form.params.join(", "), form.returns),
            form.params.clone(),
            form.documentation.clone(),
        ),
        None if ws.prelude_name(path, name) => {
            let function = ws
                .prelude_functions()
                .into_iter()
                .find(|f| f.name == name)?;
            (
                format!("{}({})", function.name, function.params.join(", ")),
                function.params,
                function.documentation,
            )
        }
        // An attribute a module declares describes itself.
        None => {
            let declared = doc.declared_attribute(name.strip_prefix('@')?)?;
            (
                format!(
                    "{name}({}) → {}",
                    declared.params.join(", "),
                    declared.applies
                ),
                declared.params.clone(),
                declared.documentation.clone(),
            )
        }
    };
    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: Some(Documentation::MarkupContent(markup(documentation))),
            parameters: Some(
                params
                    .iter()
                    .map(|p| ParameterInformation {
                        label: ParameterLabel::Simple(p.clone()),
                        documentation: None,
                    })
                    .collect(),
            ),
            active_parameter: None,
        }],
        active_signature: Some(0),
        active_parameter: (!params.is_empty())
            .then_some(argument.min(params.len().saturating_sub(1) as u32)),
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
                // After `xs |` the piped value is the first argument.
                let callee = before[..before.len() - name.len()].trim_end();
                let piped = callee.ends_with('|') && !callee.ends_with("||");
                stack.push((name, u32::from(piped)));
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
