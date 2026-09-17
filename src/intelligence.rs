//! Editor intelligence shared by the LSP handlers and deterministic tests.
use crate::{
    document::{Document, Span, byte_at},
    engine::{Engine, Value},
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
        contents: HoverContents::Markup(markup(resource.hover(path, &ws.cache))),
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
                        d.expression && d.value_span.line == row && byte >= d.value_span.start
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
    FUNCTIONS.iter().any(|f| f.name == name)
}
const FUNCTIONS: &[Function] = &[
    Function {
        name: "sum",
        params: &["table: Table", "expression: row calculation"],
        result: "Number, Money, Ratio, or Duration",
        documentation: "Evaluate the second argument for each row, then add the results. Names inside the row expression refer only to that table's columns. Example: sum(groceries, quantity * price).",
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
        name: "date",
        params: &["text: Text"],
        result: "Date or DateTime",
        documentation: "Parse an ISO date/time or a relative date. Example: date(\"next Friday\"). Relative values remain dynamic.",
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
pub fn property_names(value: &Value) -> Vec<&'static str> {
    match value {
        Value::Timer(t) => {
            let mut names = vec!["elapsed", "running", "done", "state"];
            if t.limit.is_some() {
                names.extend(["remaining", "duration"]);
            }
            names
        }
        Value::Resource(r) => {
            let mut names = vec!["url"];
            if let Some((_, kind, _)) = resources::github(&r.target) {
                names.extend(["title", "state"]);
                if kind == "pull" {
                    names.extend(["merged", "checks_passed"]);
                }
            } else if !r.target.starts_with("http") && !r.target.starts_with("geo:") {
                names.push("exists");
            }
            names
        }
        _ => vec![],
    }
}
pub fn completions(
    ws: &Workspace,
    path: &Path,
    position: Position,
    now: DateTime<FixedOffset>,
    snippets: bool,
) -> Vec<CompletionItem> {
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
    let mut engine = Engine::at(ws, now);
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
            for name in property_names(&value) {
                let preview = engine.eval(path, &format!("{receiver}.{name}"));
                result.push(CompletionItem {
                    label: name.into(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(
                        preview
                            .map(|v| format!("{} · {}", v.type_name(), v.display()))
                            .unwrap_or_else(|e| e),
                    ),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                        replacement,
                        name.into(),
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

pub fn hover(ws: &Workspace, symbol: &Symbol, now: DateTime<FixedOffset>) -> String {
    let mut engine = Engine::at(ws, now);
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
            for reference in doc.references.iter().filter(|r| {
                r.span.line == def.value_span.line && r.span.start >= def.value_span.start
            }) {
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
    match value {
        Ok(Value::Resource(r)) => {
            out.push_str(&format!("\n\n{}", r.hover(&symbol.path, &ws.cache)))
        }
        Ok(Value::Timer(t)) => {
            if let Some(limit) = t.limit {
                out.push_str(&format!(
                    "\n\n`{}`",
                    crate::charts::bar_fraction(t.elapsed as f64 / limit as f64)
                ));
            }
            out.push_str(&format!(
                "\n\nElapsed: {}. State: {}.",
                Value::Duration(t.elapsed).display(),
                t.state()
            ));
            if let Some(started) = t.started {
                out.push_str(&format!(
                    " Current segment started: {}.",
                    started.to_rfc3339()
                ));
            }
            out.push_str("\n\nUse Start / Pause / Resume / Reset timer. Controls save state in the note; ticking never edits it.");
        }
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

pub fn cell_hover(ws: &Workspace, path: &Path, position: Position) -> Option<Hover> {
    let doc = ws.documents.get(path)?;
    let byte = byte_at(doc.line(position.line as usize), position.character)?;
    for table in &doc.tables {
        for (row, cells) in table.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate().take(table.columns.len()) {
                if cell.span.line == position.line as usize
                    && byte >= cell.span.start
                    && byte <= cell.span.end
                {
                    let text = match &cell.value {
                        Ok(value) => format!(
                            "**{}.{} · {}**\n\nRow {}: {}",
                            doc.definitions[table.definition].named.name,
                            table.columns[column].name,
                            value.type_name(),
                            row + 1,
                            value.display()
                        ),
                        Err(error) => error.clone(),
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
