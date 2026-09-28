//! Completions: the names, calls and literals offered at a position.
use crate::{
    language::hover::{markup, source_link},
    language::signature::{BUILTINS, call_context},
    locate::inert,
};
use lang::common::Span;
use lang::eval::engine::Value;
use lang::model::{Document, byte_at};
use lsp_types::*;
use std::path::Path;

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
fn property_names_with_links(
    value: &Value,
    links: lang::eval::link_features::LinkFeatures<'_>,
) -> Vec<String> {
    if let Some(object) = value.host() {
        return object.fields(links);
    }
    match value {
        Value::Record(fields) => fields.keys().cloned().collect(),
        _ => value
            .kind()
            .fields()
            .iter()
            .map(|f| f.to_string())
            .collect(),
    }
}
pub(crate) fn completions(
    request: &lang::eval::RequestContext<'_>,
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
        lang::eval::tables::scope_at(doc, Span::new(position.line as usize, byte, byte))
        && let Ok(origin) = lang::eval::tables::origin(ws, path, &table_name)
        && let Some(table) = lang::eval::tables::table(ws, &origin)
    {
        for (i, column) in table.columns.iter().enumerate() {
            result.push(CompletionItem {
                label: column.name.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!(
                    "{} · column of {table_name}",
                    table.types[i].map(|t| t.as_str()).unwrap_or("Unknown")
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
        let before = &line[..start - 1];
        let named = before
            .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .unwrap_or("");
        // `import("id").` offers the library's exports: the same record a
        // note would bind, so the members are exactly `Module::public_names`.
        let imported = (named.is_empty() && before.ends_with(')'))
            .then(|| before.rfind("import(").map(|at| &before[at..]))
            .flatten();
        let receiver = imported.unwrap_or(named);
        let value = if imported.is_some() {
            engine.eval(path, receiver)
        } else {
            engine.named(path, receiver)
        };
        if let Ok(value) = value {
            let names = match &value {
                Value::Namespace(path) => ws
                    .symbols()
                    .iter()
                    .filter(|s| s.path == path.path())
                    .map(|s| ws.named(s).name.clone())
                    .collect(),
                _ => property_names_with_links(&value, request.link_features()),
            };
            for name in names {
                let preview = engine.eval(path, &format!("{receiver}.{name}"));
                result.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(crate::language::describe::summary(&preview)),
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
            let detail = crate::language::describe::detail(
                &mut engine,
                &ws.documents[&symbol.path],
                &symbol,
            );
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
    // The module-tier built-ins are not names a note has, so they are not
    // offered in one either; a module buffer still sees its own primitives.
    let module = lang::eval::modules::is_module_path(path);
    for function in BUILTINS {
        if (!module && function.tier == crate::language::signature::Tier::Module)
            || attribute != function.name.starts_with('@')
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
            detail: Some(function.result.as_str().into()),
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
    if let Some((_, _, _, title_start)) = lang::eval::itinerary::clock(line, row)
        && byte >= title_start
        && line[title_start..byte]
            .trim()
            .chars()
            .all(|c| c.is_alphabetic())
    {
        let typed = line[title_start..byte].trim();
        return Some(
            lang::eval::itinerary::KINDS
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
            lang::eval::itinerary::KEYS
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
