//! Editor intelligence shared by the LSP handlers and deterministic tests.
use crate::{
    document::{Document, Span, byte_at},
    engine::Value,
    resources,
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset};
use lsp_types::*;
use std::path::Path;

pub fn markup(value: String) -> MarkupContent {
    MarkupContent {
        kind: MarkupKind::Markdown,
        value,
    }
}
pub fn link_hover(ws: &Workspace, path: &Path, position: Position) -> Option<Hover> {
    link_hover_at(ws, path, position, chrono::Utc::now().fixed_offset())
}
pub fn link_hover_at(
    ws: &Workspace,
    path: &Path,
    position: Position,
    now: DateTime<FixedOffset>,
) -> Option<Hover> {
    link_hover_in(&crate::RequestContext::new(ws, now), path, position)
}
pub fn link_hover_in(
    request: &crate::RequestContext<'_>,
    path: &Path,
    position: Position,
) -> Option<Hover> {
    let ws = request.workspace();

    let doc = ws.documents.get(path)?;
    let byte = byte_at(doc.line(position.line as usize), position.character)?;
    let link = doc.links.iter().find(|l| {
        l.span.line == position.line as usize && l.span.start <= byte && byte < l.span.end
    })?;
    let resource = resources::Resource {
        target: link.target.clone(),
        origin: None,
    };
    Some(Hover {
        contents: HoverContents::Markup(markup(
            resource
                .presentation(
                    path,
                    &ws.cache,
                    request.now().to_utc(),
                    request.link_features(),
                )
                .hover,
        )),
        range: Some(link.span.range(&doc.text)),
    })
}
pub fn symbol_at(workspace: &Workspace, path: &Path, position: Position) -> Option<(Symbol, Span)> {
    let doc = workspace.documents.get(path)?;
    let byte = byte_at(doc.line(position.line as usize), position.character)?;
    let inside =
        |span: Span| span.line == position.line as usize && byte >= span.start && byte <= span.end;
    for (table, t) in doc.tables.iter().enumerate() {
        for (column, c) in t.columns.iter().enumerate() {
            if inside(c.span) {
                return Some((
                    Symbol {
                        path: path.into(),
                        kind: SymbolKind::Column(table, column),
                    },
                    c.span,
                ));
            }
        }
    }
    for symbol in workspace.symbols().into_iter().filter(|s| s.path == path) {
        let span = workspace.named(&symbol).span;
        if inside(span) {
            return Some((symbol, span));
        }
    }
    doc.references
        .iter()
        .find(|r| inside(Span::new(r.span.line, r.span.start, r.end())))
        .and_then(|r| {
            crate::tables::resolve_reference(workspace, path, r)
                .ok()
                .map(|s| (s, r.span))
        })
}
/// Every place a symbol appears: its declaration, references that resolve to
/// it, and for decision variables the `plan.variable` property accesses.
/// Sorted by note and position, without duplicates.
pub fn occurrences(ws: &Workspace, symbol: &Symbol) -> Vec<(std::path::PathBuf, Span)> {
    let mut found = vec![(symbol.path.clone(), ws.named(symbol).span)];
    let plan = match symbol.kind {
        SymbolKind::Variable(p, _) => Some(Symbol {
            path: symbol.path.clone(),
            kind: SymbolKind::Definition(ws.documents[&symbol.path].plans[p].definition),
        }),
        _ => None,
    };
    let name = &ws.named(symbol).name;
    for (path, doc) in &ws.documents {
        for r in &doc.references {
            if r.name == *name
                && crate::tables::resolve_reference(ws, path, r).ok().as_ref() == Some(symbol)
            {
                found.push((path.clone(), r.span));
            }
            if let (Some(plan), Some(property)) = (&plan, &r.property)
                && property == name
                && ws.resolve(path, &r.name).ok().as_ref() == Some(plan)
            {
                found.push((
                    path.clone(),
                    Span::new(r.span.line, r.span.end + 1, r.end()),
                ));
            }
        }
    }
    found.sort_by_key(|(p, s)| (p.clone(), s.line, s.start, s.end));
    found.dedup();
    // The declaration stays first so callers can treat it as the write site.
    let declaration = (symbol.path.clone(), ws.named(symbol).span);
    found.retain(|o| *o != declaration);
    found.insert(0, declaration);
    found
}
pub fn source_link(ws: &Workspace, symbol: &Symbol) -> String {
    let named = ws.named(symbol);
    let mut uri = crate::paths::file_url(&symbol.path).unwrap();
    uri.set_fragment(Some(&format!("L{}", named.span.line + 1)));
    format!("[{}](<{uri}>)", named.name)
}
pub fn inert(doc: &Document, position: Position) -> bool {
    let row = position.line as usize;
    let Some(byte) = byte_at(doc.line(row), position.character) else {
        return true;
    };
    doc.highlights.iter().any(|h| {
        h.span.line == row
            && byte >= h.span.start
            && byte < h.span.end
            && (h.kind == "comment"
                || h.kind == "string"
                    && !doc
                        .tasks
                        .iter()
                        .flat_map(|t| t.attributes.values())
                        .chain(doc.events.iter().flat_map(|e| e.attributes.values()))
                        .any(|a| a.span.line == row && byte >= a.span.start && byte <= a.span.end)
                    && !doc.definitions.iter().any(|d| {
                        d.expression && d.value_span.contains(&doc.text, Span::new(row, byte, byte))
                    }))
    })
}

struct Function {
    name: &'static str,
    params: &'static [&'static str],
    result: &'static str,
    documentation: &'static str,
    example: &'static str,
}
pub(crate) fn is_builtin_function(name: &str) -> bool {
    crate::engine::is_builtin_function(name)
}
const FUNCTIONS: &[Function] = &[
    Function {
        name: "import",
        params: &["id: Text"],
        result: "Record",
        documentation: "Load a module namespace; modules declare their imports.",
        example: "\"format\"",
    },
    Function {
        name: "solve_linear",
        params: &["model: Record"],
        result: "Record",
        documentation: "Solve a bounded linear model and return raw numeric values and status.",
        example: "model",
    },
    Function {
        name: "object",
        params: &["entries: List"],
        result: "Record",
        documentation: "Build a record from key/value pairs; duplicate keys are rejected.",
        example: "[{key: \"x\", value: 1}]",
    },
    Function {
        name: "parse_date",
        params: &["text: Text", "format: Text"],
        result: "Date or Null",
        documentation: "Parse a calendar date with a strftime format; invalid input returns null.",
        example: "\"2026-09-18\", \"%F\"",
    },
    Function {
        name: "parse_datetime",
        params: &["text: Text", "format: Text", "offset: DateTime"],
        result: "DateTime or Null",
        documentation: "Parse a local timestamp using a reference timestamp's offset.",
        example: "\"2026-09-18 09:30\", \"%F %H:%M\", now()",
    },
    Function {
        name: "entries",
        params: &["record: Record"],
        result: "List",
        documentation: "List key/value pairs in key order.",
        example: "{x: 1}",
    },
    Function {
        name: "number",
        params: &["value: Number, Money, Ratio, Duration or Count"],
        result: "Number",
        documentation: "Extract the numeric magnitude; durations use seconds.",
        example: "90m",
    },
    Function {
        name: "source",
        params: &["value: Any"],
        result: "Text",
        documentation: "Format a typed scalar as a round-trippable expression.",
        example: "now()",
    },
    Function {
        name: "make_date",
        params: &["year: Number", "month: Number", "day: Number"],
        result: "Date or Null",
        documentation: "Construct a calendar date; invalid dates return null.",
        example: "2026, 9, 18",
    },
    Function {
        name: "duration_parts",
        params: &["duration: Duration"],
        result: "Record",
        documentation: "Split integer seconds into total hours, remaining minutes and seconds without rounding.",
        example: "90m",
    },
    Function {
        name: "date_parts",
        params: &["date: Date or DateTime"],
        result: "Record",
        documentation: "Read year, month, day and weekday (Monday is zero).",
        example: "today()",
    },
    Function {
        name: "at_time",
        params: &["date: Date", "time: Duration", "offset: DateTime"],
        result: "DateTime",
        documentation: "Combine a date and time of day using the reference timestamp offset.",
        example: "today(), 9h, now()",
    },
    Function {
        name: "parse_time",
        params: &["text: Text", "format: Text"],
        result: "Duration or Null",
        documentation: "Parse a time of day as seconds since midnight.",
        example: "\"09:30\", \"%H:%M\"",
    },
    Function {
        name: "parse_duration",
        params: &["text: Text"],
        result: "Duration or Null",
        documentation: "Parse a written duration.",
        example: "\"2h\"",
    },
    Function {
        name: "pad_start",
        params: &["text: Text", "width: Number", "fill: Text"],
        result: "Text",
        documentation: "Pad text to a character width with one character.",
        example: "\"3\", 2, \"0\"",
    },
    Function {
        name: "pad_end",
        params: &["text: Text", "width: Number", "fill: Text"],
        result: "Text",
        documentation: "Pad text on the right.",
        example: "\"x\", 3, \" \"",
    },
    Function {
        name: "slice",
        params: &["value: Text or List", "start: Number", "end: Number"],
        result: "Text or List",
        documentation: "Take a half-open range; text indices count Unicode characters.",
        example: "\"hello\", 0, 2",
    },
    Function {
        name: "concat",
        params: &["lists: List..."],
        result: "List",
        documentation: "Concatenate lists.",
        example: "[1, 2], [3]",
    },
    Function {
        name: "trim",
        params: &["text: Text"],
        result: "Text",
        documentation: "Remove surrounding whitespace.",
        example: "\" hello \"",
    },
    Function {
        name: "type",
        params: &["value: Any"],
        result: "Text",
        documentation: "Get the runtime type name.",
        example: "42",
    },
    Function {
        name: "floor",
        params: &["number: Number"],
        result: "Number",
        documentation: "Round down to an integer.",
        example: "1.5",
    },
    Function {
        name: "round",
        params: &["number: Number"],
        result: "Number",
        documentation: "Round to the nearest integer.",
        example: "1.5",
    },
    Function {
        name: "repeat",
        params: &["text: Text", "count: Number"],
        result: "Text",
        documentation: "Repeat text a bounded number of times.",
        example: "\"█\", 3",
    },
    Function {
        name: "format_date",
        params: &["date: Date or DateTime", "format: Text"],
        result: "Text",
        documentation: "Format a date or timestamp with strftime directives.",
        example: "today(), \"%Y-%m-%d\"",
    },
    Function {
        name: "error",
        params: &["message: Text"],
        result: "Never",
        documentation: "Return an evaluation error.",
        example: "\"Missing data\"",
    },
    Function {
        name: "if",
        params: &["condition: Boolean", "then: Value", "else: Value"],
        result: "Value",
        documentation: "Evaluate only the selected branch.",
        example: "true, 1, 0",
    },
    Function {
        name: "coalesce",
        params: &["values: Value..."],
        result: "Value",
        documentation: "Return the first non-null value.",
        example: "null, 1",
    },
    Function {
        name: "map",
        params: &["items: List", "function: Function"],
        result: "List",
        documentation: "Apply a pure function to every item.",
        example: "[1, 2], fn(x) => x * 2",
    },
    Function {
        name: "filter",
        params: &["items: List", "predicate: Function"],
        result: "List",
        documentation: "Keep items whose predicate returns true.",
        example: "[1, 2], fn(x) => x > 1",
    },
    Function {
        name: "sort_by",
        params: &["items: List", "key: Function"],
        result: "List",
        documentation: "Stable ascending sort by a compatible scalar key; nulls come last.",
        example: "[3, 1], fn(x) => x",
    },
    Function {
        name: "group_by",
        params: &["items: List", "key: Function"],
        result: "List",
        documentation: "Group by a scalar key into {key, rows} records, in first-seen order.",
        example: "[1, 2, 1], fn(x) => x",
    },
    Function {
        name: "eval",
        params: &["expression: Text"],
        result: "Value",
        documentation: "Evaluate expression text in the current document's scope.",
        example: "\"price * 2\"",
    },
    Function {
        name: "fold",
        params: &["items: List", "initial: Value", "function: Function"],
        result: "Value",
        documentation: "Combine items left to right with an accumulator.",
        example: "[1, 2], 0, fn(a, x) => a + x",
    },
    Function {
        name: "get",
        params: &["collection: Record or List", "key: Text or Number"],
        result: "Value",
        documentation: "Read a field or index; return null when absent.",
        example: "{name: \"hello\"}, \"name\"",
    },
    Function {
        name: "length",
        params: &["value: List, Record, or Text"],
        result: "Count",
        documentation: "Count items, fields, or Unicode characters.",
        example: "\"hello\"",
    },
    Function {
        name: "text",
        params: &["value: Value"],
        result: "Text",
        documentation: "Format a value as text; null remains null.",
        example: "$25",
    },
    Function {
        name: "contains",
        params: &["value: List or Text", "part: Value"],
        result: "Boolean",
        documentation: "Test membership or a text substring.",
        example: "\"hello\", \"ell\"",
    },
    Function {
        name: "starts_with",
        params: &["text: Text", "prefix: Text"],
        result: "Boolean",
        documentation: "Test a text prefix.",
        example: "\"hello\", \"he\"",
    },
    Function {
        name: "ends_with",
        params: &["text: Text", "suffix: Text"],
        result: "Boolean",
        documentation: "Test a text suffix.",
        example: "\"hello\", \"lo\"",
    },
    Function {
        name: "split",
        params: &["text: Text", "separator: Text"],
        result: "List",
        documentation: "Split text into pieces.",
        example: "\"a/b\", \"/\"",
    },
    Function {
        name: "join",
        params: &["items: List", "separator: Text"],
        result: "Text",
        documentation: "Join a list of text.",
        example: "[\"a\", \"b\"], \"/\"",
    },
    Function {
        name: "lower",
        params: &["text: Text"],
        result: "Text",
        documentation: "Convert text to lowercase.",
        example: "\"Hello\"",
    },
    Function {
        name: "upper",
        params: &["text: Text"],
        result: "Text",
        documentation: "Convert text to uppercase.",
        example: "\"Hello\"",
    },
    Function {
        name: "replace",
        params: &["text: Text", "from: Text", "to: Text"],
        result: "Text",
        documentation: "Replace text occurrences.",
        example: "\"hello\", \"h\", \"j\"",
    },
    Function {
        name: "sum",
        params: &["items: List or Table", "expression?: row calculation"],
        result: "Number, Money, Ratio, or Duration",
        documentation: "Sum a list of compatible quantities, skipping nulls, or evaluate a row expression for each table row and add the results. Units are preserved.",
        example: "groceries, quantity * price",
    },
    Function {
        name: "countdown",
        params: &[
            "duration: Duration",
            "elapsed?: Duration",
            "started?: DateTime",
        ],
        result: "Countdown",
        documentation: "An idle countdown. Use Start timer to capture a timestamp; elapsed and started are persisted by timer controls.",
        example: "25m",
    },
    Function {
        name: "stopwatch",
        params: &["elapsed?: Duration", "started?: DateTime"],
        result: "Stopwatch",
        documentation: "An idle stopwatch. Use Start, Pause, Resume, or Reset timer. Elapsed time includes time while the editor is closed.",
        example: "",
    },
    Function {
        name: "maximize",
        params: &["objective: linear expression"],
        result: "Plan",
        documentation: "Declare a linear plan: the next lines hold a | constraint | expression | table. Names no note defines become decision variables (at least zero); everything else is a constant. Example: maximize(3 * bagels + 1.25 * doughnuts).",
        example: "3 * bagels + 1.25 * doughnuts",
    },
    Function {
        name: "solve",
        params: &["constraint: expression with <=, >=, or =="],
        result: "Number, Money, or Duration",
        documentation: "Goal seek: the definition's own name is the unknown, and the answer is the boundary value that makes the constraint hold through any chain of calculations. Example: [monthly] := solve(saved_by_june >= $5,000).",
        example: "total >= $500",
    },
    Function {
        name: "minimize",
        params: &["objective: linear expression"],
        result: "Plan",
        documentation: "Like maximize, but finds the smallest objective that satisfies every constraint in the table below.",
        example: "cost",
    },
    Function {
        name: "today",
        params: &[],
        result: "Date",
        documentation: "The current local calendar date. Updates at midnight.",
        example: "",
    },
    Function {
        name: "now",
        params: &[],
        result: "DateTime",
        documentation: "The current timestamp. Sampled once per evaluation; live hints refresh every second.",
        example: "",
    },
    Function {
        name: "rate",
        params: &["from: currency code", "to: currency code"],
        result: "Number",
        documentation: "The cached exchange rate between two currencies, e.g. rate(EUR, USD). Refresh with wtf refresh or the Refresh lookups lens; hovers show the age.",
        example: "EUR, USD",
    },
    Function {
        name: "to",
        params: &["amount: Money", "currency: code"],
        result: "Money",
        documentation: "Convert money using the cached rate, e.g. to(hotel, USD). Money in different currencies never adds up silently.",
        example: "hotel, USD",
    },
    Function {
        name: "forecast",
        params: &["place: Text", "date: Date", "unit?: F or C"],
        result: "Forecast",
        documentation: "The cached forecast for a place and day, with .high, .low, .summary and .rain. Itinerary days with a place get one automatically.",
        example: "\"Oaxaca\", 2026-11-20",
    },
    Function {
        name: "quote",
        params: &["symbol: ticker code"],
        result: "Money",
        documentation: "The cached last price for a ticker, e.g. quote(NVDA). The built-in source covers US tickers; set a quote provider in .wtf/providers.json for others.",
        example: "NVDA",
    },
    Function {
        name: "date",
        params: &["value: Text, Date, or DateTime"],
        result: "Date or DateTime",
        documentation: "Parse ISO or relative date text, or take a timestamp's calendar date in the request timezone.",
        example: "\"next Friday\"",
    },
    Function {
        name: "effort",
        params: &["checklist: Checklist"],
        result: "Duration",
        documentation: "Sum estimates of unfinished leaf tasks beneath a named heading.",
        example: "checklist",
    },
    Function {
        name: "total",
        params: &["checklist: Checklist"],
        result: "Count",
        documentation: "Count all leaf tasks beneath a named heading.",
        example: "checklist",
    },
    Function {
        name: "completed",
        params: &["checklist: Checklist"],
        result: "Count",
        documentation: "Count completed leaf tasks beneath a named heading.",
        example: "checklist",
    },
    Function {
        name: "remaining",
        params: &["checklist: Checklist"],
        result: "Count",
        documentation: "Count unfinished leaf tasks beneath a named heading.",
        example: "checklist",
    },
    Function {
        name: "@timer",
        params: &["timer: Timer"],
        result: "task attribute",
        documentation: "Associate a named timer with this task. Completion does not stop the timer.",
        example: "focus",
    },
    Function {
        name: "@due",
        params: &["date: Date or DateTime"],
        result: "task attribute",
        documentation: "Deadline. Accepts a named date, an expression, or relative input such as tomorrow. Use Freeze relative date to capture it.",
        example: "tomorrow",
    },
    Function {
        name: "@scheduled",
        params: &["date: Date or DateTime"],
        result: "task attribute",
        documentation: "Planned work date; separate from the deadline.",
        example: "tomorrow",
    },
    Function {
        name: "@at",
        params: &["time: Date or DateTime"],
        result: "appointment attribute",
        documentation: "Appointment time. Include an explicit UTC offset for ambiguous local times.",
        example: "2026-09-18T14:00-04:00",
    },
    Function {
        name: "@estimate",
        params: &["effort: Duration"],
        result: "task attribute",
        documentation: "A nonnegative estimate. Examples: 30s, 20m, 2h.",
        example: "20m",
    },
    Function {
        name: "@after",
        params: &["dependency: Boolean or Checklist", "more dependencies..."],
        result: "task attribute",
        documentation: "Block this task until all dependencies are satisfied. Cycles are reported with source locations.",
        example: "task_name",
    },
    Function {
        name: "@every",
        params: &["interval: recurrence"],
        result: "task attribute",
        documentation: "Repeat a leaf task: day, week, month, year, or a positive whole-day duration such as 2w.",
        example: "week",
    },
    Function {
        name: "@tag",
        params: &["tag: name"],
        result: "task attribute",
        documentation: "Tag a task for filtering. Multiple tags may be comma-separated.",
        example: "errands",
    },
];

/// Tolerates incomplete calls, quoted strings and nested parentheses.
pub fn call_context(prefix: &str) -> Option<(String, u32)> {
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
pub fn signature(doc: &Document, position: Position) -> Option<SignatureHelp> {
    if inert(doc, position) {
        return None;
    }
    let line = doc.line(position.line as usize);
    let byte = byte_at(line, position.character)?;
    let (name, argument) = call_context(&line[..byte])?;
    let function = FUNCTIONS.iter().find(|f| f.name == name)?;
    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: format!(
                "{}({}) → {}",
                function.name,
                function.params.join(", "),
                function.result
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
fn accepts(context: Option<&(String, u32)>, value: &Value) -> bool {
    match context.map(|(name, index)| (name.as_str(), *index)) {
        Some(("sum", 0)) => matches!(value, Value::Table(_)),
        Some(("@timer", _)) => matches!(value, Value::Timer(t) if t.origin.is_some()),
        Some(("@due" | "@scheduled" | "@at", _)) => {
            matches!(value, Value::Date(_) | Value::DateTime(_))
        }
        Some(("@estimate" | "countdown", 0)) | Some(("stopwatch", 0)) | Some(("countdown", 1)) => {
            matches!(value, Value::Duration(_))
        }
        Some(("stopwatch", 1)) | Some(("countdown", 2)) => matches!(value, Value::DateTime(_)),
        Some(("effort" | "total" | "remaining" | "completed", _)) => {
            matches!(value, Value::Tasks(_))
        }
        Some(("@after", _)) => matches!(value, Value::Bool(_) | Value::Tasks(_)),
        Some(("date", _)) => matches!(value, Value::Text(_)),
        Some(("@every" | "@tag", _)) => false,
        _ => true,
    }
}
pub fn property_names(value: &Value) -> Vec<String> {
    property_names_with_links(value, crate::link_features::BUILTINS)
}
fn property_names_with_links(
    value: &Value,
    links: crate::link_features::LinkFeatures<'_>,
) -> Vec<String> {
    let names: Vec<&str> = match value {
        Value::Timer(t) => {
            let mut names = vec!["elapsed", "running", "done", "state"];
            if t.limit.is_some() {
                names.extend(["remaining", "duration"]);
            }
            names
        }
        Value::Resource(r) => {
            let mut names = vec!["url".to_owned()];
            names.extend(links.property_names(&r.target));
            if !r.target.starts_with("http") && !r.target.starts_with("geo:") {
                names.push("exists".into());
            }
            return names;
        }
        Value::Record(fields) => return fields.keys().cloned().collect(),
        Value::Plan(p) => return p.property_names(),
        _ => vec![],
    };
    names.into_iter().map(str::to_string).collect()
}
pub fn completions(
    ws: &Workspace,
    path: &Path,
    position: Position,
    now: DateTime<FixedOffset>,
    snippets: bool,
) -> Vec<CompletionItem> {
    completions_in(
        &crate::RequestContext::new(ws, now),
        path,
        position,
        snippets,
    )
}
pub fn completions_in(
    request: &crate::RequestContext<'_>,
    path: &Path,
    position: Position,
    snippets: bool,
) -> Vec<CompletionItem> {
    let ws = request.workspace();
    let now = request.now();

    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    if inert(doc, position) {
        return vec![];
    }
    let line = doc.line(position.line as usize);
    let Some(byte) = byte_at(line, position.character) else {
        return vec![];
    };
    let start = line[..byte]
        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map(|i| i + line[i..].chars().next().unwrap().len_utf8())
        .unwrap_or(0);
    let end = byte
        + line[byte..]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
    let replacement = Span::new(position.line as usize, start, end).range(&doc.text);
    let mut engine = request.engine();
    let mut result = vec![];
    if let Some(table_name) =
        crate::tables::scope_at(doc, Span::new(position.line as usize, byte, byte))
        && let Ok(origin) = crate::tables::origin(ws, path, &table_name)
        && let Some(table) = crate::tables::table(ws, &origin)
    {
        for (i, column) in table.columns.iter().enumerate() {
            result.push(CompletionItem {
                label: column.name.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!(
                    "{} · column of {table_name}",
                    table.types[i].unwrap_or("Unknown")
                )),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                    replacement,
                    column.name.clone(),
                ))),
                ..Default::default()
            });
        }
        return result;
    }
    if start > 0 && line.as_bytes()[start - 1] == b'.' {
        let receiver = line[..start - 1]
            .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .unwrap_or("");
        if let Ok(value) = engine.named(path, receiver) {
            for name in property_names_with_links(&value, request.link_features()) {
                let preview = engine.eval(path, &format!("{receiver}.{name}"));
                result.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(
                        preview
                            .map(|v| format!("{} · {}", v.type_name(), v.display()))
                            .unwrap_or_else(|e| e),
                    ),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                        replacement,
                        name.clone(),
                    ))),
                    ..Default::default()
                });
            }
        }
        return result;
    }
    let attribute = start > 0 && line.as_bytes()[start - 1] == b'@';
    let context = call_context(&line[..byte]);
    if !attribute {
        for symbol in ws.symbols() {
            let name = &ws.named(&symbol).name;
            if ws.resolve(path, name).ok().as_ref() != Some(&symbol) {
                continue;
            }
            let value = engine.symbol(&symbol);
            if context.is_some() && !value.as_ref().is_ok_and(|v| accepts(context.as_ref(), v)) {
                continue;
            }
            let detail = value
                .map(|v| format!("{} · {}", v.type_name(), v.display()))
                .unwrap_or_else(|e| e);
            result.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(detail.chars().take(120).collect()),
                documentation: Some(Documentation::MarkupContent(markup(format!(
                    "Defined in {}\n\n{}",
                    symbol.path.display(),
                    source_link(ws, &symbol)
                )))),
                sort_text: Some(format!(
                    "{}_{name}",
                    if symbol.path == path { "0" } else { "1" }
                )),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                    replacement,
                    name.clone(),
                ))),
                ..Default::default()
            });
        }
    }
    if let Some(items) = itinerary_completions(doc, position, replacement) {
        return items;
    }
    let prose = line[..byte]
        .rfind('[')
        .is_some_and(|i| !line[i..byte].contains(']'));
    for function in FUNCTIONS {
        if attribute != function.name.starts_with('@')
            || (!attribute && (prose || context.as_ref().is_some_and(|(n, _)| n == "@timer")))
        {
            continue;
        }
        if let Some((name, _)) = &context {
            let allowed = match name.as_str() {
                "@due" | "@scheduled" | "@at" => matches!(function.name, "date" | "today" | "now"),
                "@estimate" | "countdown" | "stopwatch" => match function.name {
                    "effort" => accepts(context.as_ref(), &Value::Duration(0)),
                    "now" => accepts(context.as_ref(), &Value::DateTime(now)),
                    _ => false,
                },
                "@after" => false,
                "effort" | "total" | "remaining" | "completed" | "date" | "@every" | "@tag" => {
                    false
                }
                _ => true,
            };
            if !allowed {
                continue;
            }
        }
        let name = function.name.trim_start_matches('@');
        let already_open = line[end..].starts_with('(');
        let text = if already_open {
            name.into()
        } else if snippets && !function.example.is_empty() {
            format!("{name}(${{1:{}}})$0", function.example)
        } else {
            format!("{name}({})", function.example)
        };
        result.push(CompletionItem {
            label: format!("{name}({})", function.example),
            kind: Some(if attribute {
                CompletionItemKind::KEYWORD
            } else {
                CompletionItemKind::FUNCTION
            }),
            filter_text: Some(name.into()),
            detail: Some(function.result.into()),
            documentation: Some(Documentation::MarkupContent(markup(
                function.documentation.into(),
            ))),
            insert_text_format: Some(if snippets && !already_open {
                InsertTextFormat::SNIPPET
            } else {
                InsertTextFormat::PLAIN_TEXT
            }),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(replacement, text))),
            ..Default::default()
        });
    }
    let literals: &[&str] = match context.as_ref().map(|c| c.0.as_str()) {
        Some("@due" | "@scheduled" | "@at") => &["today", "tomorrow", "next Friday"],
        Some("@estimate" | "countdown" | "stopwatch")
            if accepts(context.as_ref(), &Value::Duration(0)) =>
        {
            &["0s", "30s", "5m", "25m", "1h"]
        }
        Some("@every") => &["day", "week", "month", "year", "2w"],
        _ => &[],
    };
    for name in literals {
        result.push(CompletionItem {
            label: (*name).into(),
            kind: Some(CompletionItemKind::VALUE),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                replacement,
                (*name).into(),
            ))),
            ..Default::default()
        });
    }
    result
}

/// After a time on an itinerary line, offer stop kinds; at the start of a
/// line inside a stop, offer detail keys.
fn itinerary_completions(
    doc: &Document,
    position: Position,
    replacement: Range,
) -> Option<Vec<CompletionItem>> {
    let row = position.line as usize;
    let line = doc.line(row);
    let byte = byte_at(line, position.character)?;
    let day = doc
        .days
        .iter()
        .find(|d| d.line < row && row < d.end_line.max(row + 1) && d.line != row)?;
    let item =
        |label: String, insert: String, detail: &str, kind: CompletionItemKind| CompletionItem {
            label,
            kind: Some(kind),
            detail: Some(detail.into()),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(replacement, insert))),
            ..Default::default()
        };
    if let Some((_, _, _, title_start)) = crate::itinerary::clock(line, row)
        && byte >= title_start
        && line[title_start..byte]
            .trim()
            .chars()
            .all(|c| c.is_alphabetic())
    {
        let typed = line[title_start..byte].trim();
        return Some(
            crate::itinerary::KINDS
                .iter()
                .filter(|kind| {
                    typed.is_empty() || kind.name.to_lowercase().starts_with(&typed.to_lowercase())
                })
                .map(|kind| {
                    item(
                        format!("{} {}", kind.marker, kind.name),
                        format!("{} {} ", kind.marker, kind.name),
                        "itinerary stop",
                        CompletionItemKind::EVENT,
                    )
                })
                .collect(),
        );
    }
    let in_stop = day
        .stops
        .iter()
        .any(|s| s.line < row && row < s.end_line.max(row + 1));
    if in_stop
        && line[..byte].trim().chars().all(|c| c.is_alphabetic())
        && !line[byte..].contains(':')
    {
        let typed = line[..byte].trim().to_lowercase();
        return Some(
            crate::itinerary::KEYS
                .iter()
                .filter(|k| typed.is_empty() || k.to_lowercase().starts_with(&typed))
                .map(|k| {
                    item(
                        format!("{k}:"),
                        format!("{k}: "),
                        "stop detail",
                        CompletionItemKind::PROPERTY,
                    )
                })
                .collect(),
        );
    }
    None
}
pub fn hover(ws: &Workspace, symbol: &Symbol, now: DateTime<FixedOffset>) -> String {
    hover_in(&crate::RequestContext::new(ws, now), symbol)
}
pub fn hover_in(request: &crate::RequestContext<'_>, symbol: &Symbol) -> String {
    let ws = request.workspace();
    let now = request.now();
    let features = request.link_features();
    let mut engine = request.engine();
    let named = ws.named(symbol);
    if let SymbolKind::Column(t, c) = symbol.kind {
        let doc = &ws.documents[&symbol.path];
        let table = &doc.tables[t];
        let name = &doc.definitions[table.definition].named.name;
        let samples = table
            .rows
            .iter()
            .filter_map(|r| r.get(c))
            .take(8)
            .map(|cell| {
                cell.value
                    .as_ref()
                    .map(Value::display)
                    .unwrap_or_else(|e| e.clone())
            })
            .collect::<Vec<_>>()
            .join(", ");
        let values: Vec<Value> = table
            .rows
            .iter()
            .filter_map(|r| r.get(c))
            .filter_map(|cell| cell.value.clone().ok())
            .collect();
        if let Some(domain) = table.domains[c] {
            return format!(
                "**{} · {}**\n\nDecision column of `{name}` ({}): a plan that sums over it chooses {} for every row. Written cell values are notes; the plan's inlays show the choice.\n\nDefinition: {}",
                named.name,
                domain.type_name(),
                match domain {
                    crate::tables::Domain::Choice => "name?",
                    crate::tables::Domain::Count => "name#",
                },
                match domain {
                    crate::tables::Domain::Choice => "yes or no",
                    crate::tables::Domain::Count => "a whole number",
                },
                source_link(ws, symbol)
            );
        }
        let chart = crate::charts::series(&values)
            .map(|chart| format!("\n\n{chart}"))
            .unwrap_or_default();
        return format!(
            "**{} · {}**\n\nColumn of `{name}` · {} rows{chart}\n\nValues: {samples}\n\nDefinition: {}",
            named.name,
            table.types[c].unwrap_or("Unknown"),
            table.rows.len(),
            source_link(ws, symbol)
        );
    }
    let value = engine.symbol(symbol);
    let mut out = match &value {
        Ok(v) => format!("**{} · {}**\n\n{}", named.name, v.type_name(), v.display()),
        Err(e) => format!("**{}**\n\n{e}", named.name),
    };
    if let SymbolKind::Variable(p, _) = symbol.kind {
        let plan = Symbol {
            path: symbol.path.clone(),
            kind: SymbolKind::Definition(ws.documents[&symbol.path].plans[p].definition),
        };
        out.push_str(&format!(
            "\n\nDecision variable of {}: no note defines this name, so the plan chooses its value.",
            source_link(ws, &plan)
        ));
    }
    if let SymbolKind::Definition(i) = symbol.kind {
        let def = &ws.documents[&symbol.path].definitions[i];
        if def.expression && def.source != "table" {
            let substituted = engine
                .substituted(&symbol.path, &def.source)
                .unwrap_or_else(|_| def.source.clone());
            out.push_str(&format!(
                "\n\n```text\n{}\n",
                def.source.replace('`', "\\`")
            ));
            if substituted != def.source {
                out.push_str(&format!("= {substituted}\n"));
            }
            if let Ok(v) = &value {
                out.push_str(&format!("= {}\n", v.display()));
            }
            out.push_str("```");
            if let Some(body) = crate::plans::seek_body(&def.source) {
                let vars = [named.name.clone()].into_iter().collect();
                if let Ok((lhs, op, rhs)) =
                    engine.constraint(&symbol.path, body, def.value_span, &vars)
                    && let Ok(difference) = lhs.minus(&rhs)
                {
                    let coefficient = difference.terms.get(&named.name).copied().unwrap_or(0.0);
                    out.push_str(&format!(
                        "\n\nGoal seek: {} `{body}`.",
                        engine
                            .call_module(
                                "plan",
                                "seek_summary",
                                vec![Value::Text(op), Value::Bool(coefficient > 0.0)]
                            )
                            .map(|v| v.display())
                            .unwrap_or_else(|e| e)
                    ));
                }
            }
            if let Ok(Value::Plan(plan)) = &value
                && let Ok(text) = ws.modules.call("plan", "hover", vec![plan.record(ws)], now)
            {
                out.push_str(&text.display());
            }
            if let Some(contributions) = engine.sum_contributions(&symbol.path, &def.source) {
                out.push_str("\n\nRow contributions:\n");
                if let Some(chart) = crate::charts::series(&contributions) {
                    out.push_str(&format!("\n{chart}\n"));
                }
                for (row, value) in contributions.iter().enumerate().take(30) {
                    out.push_str(&format!("\n- Row {}: {}", row + 1, value.display()));
                }
                if contributions.len() > 30 {
                    out.push_str(&format!("\n- … {} more rows", contributions.len() - 30));
                }
            }
            let doc = &ws.documents[&symbol.path];
            let mut inputs = std::collections::BTreeSet::new();
            for reference in doc
                .references
                .iter()
                .filter(|r| def.value_span.contains(&doc.text, r.span))
            {
                if let Ok(input) = crate::tables::resolve_reference(ws, &symbol.path, reference) {
                    inputs.insert(source_link(ws, &input));
                }
            }
            if !inputs.is_empty() {
                out.push_str(&format!(
                    "\n\nInputs: {}",
                    inputs.into_iter().collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
    if !engine.wanted.is_empty() {
        let mut keys = engine.wanted.clone();
        keys.sort();
        keys.dedup();
        let now = now.to_utc();
        out.push_str("\n\nLookups:");
        for key in keys {
            match ws.lookups.get(&key) {
                Some(lookup) => out.push_str(&format!(
                    "\n- {} · {} · {}",
                    crate::lookups::describe(&key),
                    crate::resources::ago(lookup.fetched_at, now),
                    lookup.source
                )),
                None => out.push_str(&format!(
                    "\n- {} · not fetched yet",
                    crate::lookups::describe(&key)
                )),
            }
        }
    }
    match value {
        Ok(Value::Resource(r)) => out.push_str(&format!(
            "\n\n{}",
            r.presentation(&symbol.path, &ws.cache, now.to_utc(), features)
                .hover
        )),
        Ok(Value::Timer(t)) => out.push_str(&t.hover()),
        Ok(Value::Tasks(tasks)) => {
            let done = tasks
                .iter()
                .filter(|(p, i)| engine.task_done(p, *i))
                .count();
            out.push_str(&format!(
                "\n\n`{}` {done}/{} complete",
                crate::charts::bar(done, tasks.len()),
                tasks.len()
            ));
        }
        _ => {}
    }
    out.push_str(&format!(
        "\n\nDefinition: {} · {}:{}",
        source_link(ws, symbol),
        symbol.path.display(),
        named.span.line + 1
    ));
    out
}

/// A bracketed calculation in prose: its expression, substitution and value.
pub fn calculation_hover(
    ws: &Workspace,
    path: &Path,
    position: Position,
    now: DateTime<FixedOffset>,
) -> Option<Hover> {
    calculation_hover_in(&crate::RequestContext::new(ws, now), path, position)
}
pub fn calculation_hover_in(
    request: &crate::RequestContext<'_>,
    path: &Path,
    position: Position,
) -> Option<Hover> {
    let ws = request.workspace();

    let doc = ws.documents.get(path)?;
    let byte = byte_at(doc.line(position.line as usize), position.character)?;
    let calculation = doc.calculations.iter().find(|c| {
        c.span.line == position.line as usize
            && byte + usize::from(c.bracketed) >= c.span.start
            && byte <= c.span.end
    })?;
    let mut engine = request.engine();
    let value = engine.eval_at(path, &calculation.source, calculation.span);
    let mut text = match &value {
        Ok(v) => format!("**{} · {}**", v.display(), v.type_name()),
        Err(e) => format!("**Calculation**\n\n{e}"),
    };
    // Line calculations keep spaces where their brackets were; show them tidy.
    let tidy = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    text.push_str(&format!(
        "\n\n```text\n{}\n",
        tidy(&calculation.source).replace('`', "\\`")
    ));
    if let Ok(substituted) = engine.substituted(path, &calculation.source)
        && substituted != calculation.source
    {
        text.push_str(&format!("= {}\n", tidy(&substituted)));
    }
    if let Ok(v) = &value {
        text.push_str(&format!("= {}\n", v.display()));
    }
    text.push_str("```");
    Some(Hover {
        contents: HoverContents::Markup(markup(text)),
        range: Some(
            Span::new(
                calculation.span.line,
                calculation.span.start - usize::from(calculation.bracketed),
                calculation.span.end + usize::from(calculation.bracketed),
            )
            .range(&doc.text),
        ),
    })
}
pub fn cell_hover(ws: &Workspace, path: &Path, position: Position) -> Option<Hover> {
    cell_hover_in(
        &crate::RequestContext::new(ws, chrono::Local::now().fixed_offset()),
        path,
        position,
    )
}
pub fn cell_hover_in(
    request: &crate::RequestContext<'_>,
    path: &Path,
    position: Position,
) -> Option<Hover> {
    let ws = request.workspace();

    let doc = ws.documents.get(path)?;
    let byte = byte_at(doc.line(position.line as usize), position.character)?;
    for table in &doc.tables {
        for (row, cells) in table.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate().take(table.columns.len()) {
                if cell.span.line == position.line as usize
                    && byte >= cell.span.start
                    && byte <= cell.span.end
                {
                    let value = match &cell.expression {
                        Some((inner, _)) => request
                            .engine()
                            .eval(path, inner)
                            .map(|v| (v, Some(inner.clone()))),
                        None => cell.value.clone().map(|v| (v, None)),
                    };
                    let text = match value {
                        Ok((value, expression)) => format!(
                            "**{}.{} · {}**\n\nRow {}: {}{}",
                            doc.definitions[table.definition].named.name,
                            table.columns[column].name,
                            value.type_name(),
                            row + 1,
                            value.display(),
                            expression
                                .map(|e| format!("\n\nCalculated from `{e}`"))
                                .unwrap_or_default()
                        ),
                        Err(error) => error,
                    };
                    return Some(Hover {
                        contents: HoverContents::Markup(markup(text)),
                        range: Some(cell.span.range(&doc.text)),
                    });
                }
            }
        }
    }
    None
}
