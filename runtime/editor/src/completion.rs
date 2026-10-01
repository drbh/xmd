//! Completions: the names, calls and literals offered at a position.
use analysis::{BUILTINS, call_context, describe, inert, markup, source_link};
use lang::common::Span;
use lang::eval::engine::{HostPresenting, Tier, Value, claimed};
use lang::eval::modules::ModuleKind;
use lang::eval::tables::TableValue;
use lang::model::{Document, byte_at};
use lang::syntax::AttributeValue;
use lsp_types::*;
use std::path::Path;

/// What the value of an attribute holds, when a call context names one: a
/// native attribute or one a module declares.
fn attribute_value(doc: &Document, name: &str) -> Option<AttributeValue> {
    doc.attribute_value(name.strip_prefix('@')?)
}
/// The kind the argument at a call context takes, when the call is one the
/// note makes to a prelude function that declares its arguments' kinds
/// (`accepts` in its manifest): `Some(None)` past the last one it declares.
fn declared_kind(
    ws: &lang::eval::Workspace,
    path: &Path,
    context: Option<(&str, u32)>,
) -> Option<Option<String>> {
    let (name, index) = context?;
    if !ws.prelude_name(path, name) {
        return None;
    }
    let function = ws
        .prelude_functions()
        .into_iter()
        .find(|f| f.name == name)?;
    (!function.accepts.is_empty()).then(|| function.accepts.get(index as usize).cloned())
}
fn accepts(
    doc: &Document,
    context: Option<(&str, u32)>,
    kind: Option<&Option<String>>,
    value: &Value,
) -> bool {
    // Inside an attribute, what its value holds decides what fits.
    if let Some(declared) =
        context.and_then(|(name, _)| doc.declared_attribute(name.strip_prefix('@')?))
    {
        return match declared.value {
            AttributeValue::When => matches!(value, Value::Date(_) | Value::DateTime(_)),
            AttributeValue::Duration => matches!(value, Value::Duration(_)),
            AttributeValue::Dependencies => matches!(value, Value::Bool(_) | Value::Tasks(_)),
            // A tagged record of a kind it names, which its own definition made.
            AttributeValue::Tagged => claimed(value, &declared.kinds),
            AttributeValue::Expression => true,
            // Literal text or a stamped date: no name fits.
            AttributeValue::Date | AttributeValue::Text => false,
        };
    }
    // Inside a prelude call, the kind it declares for the argument.
    if let Some(kind) = kind {
        return kind.as_deref().is_none_or(|kind| value.type_name() == kind);
    }
    match context {
        Some(("sum", 0)) => value.downcast::<TableValue>().is_some(),
        Some(("date", _)) => matches!(value, Value::Text(_)),
        _ => true,
    }
}
/// A call or attribute completion offers: a built-in's row, or an
/// attribute a module declares.
struct Offered<'a> {
    name: String,
    example: &'a str,
    result: &'a str,
    documentation: &'a str,
    tier: Tier,
}

fn property_names_with_links(
    value: &Value,
    links: lang::eval::link_features::LinkFeatures<'_>,
) -> Vec<String> {
    if value.host().is_some() {
        return value.host_fields(links);
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
    request: &crate::Request<'_>,
    path: &Path,
    position: Position,
    snippets: bool,
) -> Vec<CompletionItem> {
    let ws = request.workspace();

    let Some(doc) = ws.documents().get(path) else {
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
    let replacement = Span::new(position.line as usize, start, end).range(doc);
    if let Some(items) =
        column_completions(request, path, position.line as usize, byte, replacement)
    {
        return items;
    }
    if start > 0 && line.as_bytes()[start - 1] == b'.' {
        return property_completions(request, path, &line[..start - 1], replacement);
    }
    // A feature module that offers something here answers for the position.
    if let Some(offered) = request
        .workspace()
        .modules()
        .of_kind(ModuleKind::Feature)
        .find_map(|m| crate::modules::completions(m, request, path, position))
    {
        return offered
            .into_iter()
            .map(|item| CompletionItem {
                label: item.label,
                kind: item.kind,
                detail: item.detail,
                text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                    replacement,
                    item.insert,
                ))),
                ..Default::default()
            })
            .collect();
    }
    let mut engine = request.engine();
    let mut result = vec![];
    let attribute = start > 0 && line.as_bytes()[start - 1] == b'@';
    let context = call_context(&line[..byte]);
    let kind = declared_kind(ws, path, context);
    if !attribute {
        for symbol in ws.symbols() {
            let name = &ws.named(&symbol).name;
            if ws.resolve(path, name).ok().as_ref() != Some(&symbol) {
                continue;
            }
            let value = engine.symbol(&symbol);
            if context.is_some()
                && !value
                    .as_ref()
                    .is_ok_and(|v| accepts(doc, context, kind.as_ref(), v))
            {
                continue;
            }
            let detail = describe::detail(&mut engine, &ws.documents()[&symbol.path], &symbol);
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
    // The module-tier built-ins are not names a note has, so they are not
    // offered in one either; a module buffer still sees its own primitives.
    let module = lang::eval::modules::is_module_path(path);
    // The built-ins and task attributes, then the attributes modules declare.
    let declared: Vec<Offered<'_>> = doc
        .declarations()
        .iter()
        .map(|d| Offered {
            name: format!("@{}", d.key),
            example: &d.example,
            result: &d.applies,
            documentation: &d.documentation,
            tier: Tier::Note,
        })
        .collect();
    let offered = BUILTINS
        .iter()
        .map(|f| Offered {
            name: f.name.into(),
            example: f.example,
            result: f.result.as_str(),
            documentation: f.documentation,
            tier: f.tier,
        })
        .chain(declared);
    for function in offered {
        if (!module && function.tier == Tier::Module)
            || attribute != function.name.starts_with('@')
            || (!attribute
                && (prose
                    || context.is_some_and(|(n, _)| {
                        attribute_value(doc, n) == Some(AttributeValue::Tagged)
                    })))
        {
            continue;
        }
        if let Some((name, _)) = context {
            let allowed = match attribute_value(doc, name) {
                Some(AttributeValue::When) => {
                    matches!(function.name.as_str(), "date" | "today" | "now")
                }
                Some(AttributeValue::Duration) => function.result == "Duration",
                Some(_) => false,
                // Inside a prelude call that declares the argument's kind,
                // the built-ins that answer with it.
                None if kind.is_some() => {
                    kind.as_ref().and_then(Option::as_deref) == Some(function.result)
                }
                None => name != "date",
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
    // The prelude's functions, wherever a call that is no special form's
    // argument may go.
    let generic = kind.is_none()
        && context.is_none_or(|(name, _)| attribute_value(doc, name).is_none() && name != "date");
    if !attribute && !prose && generic {
        for function in ws.prelude_functions() {
            if !ws.prelude_name(path, &function.name) {
                continue;
            }
            let params = function.params.join(", ");
            let already_open = line[end..].starts_with('(');
            let text = if already_open {
                function.name.clone()
            } else if snippets && !params.is_empty() {
                format!("{}(${{1:{params}}})$0", function.name)
            } else {
                format!("{}()", function.name)
            };
            result.push(CompletionItem {
                label: format!("{}({params})", function.name),
                kind: Some(CompletionItemKind::FUNCTION),
                filter_text: Some(function.name.clone()),
                detail: Some("prelude".into()),
                documentation: Some(Documentation::MarkupContent(markup(function.documentation))),
                insert_text_format: Some(if snippets && !already_open {
                    InsertTextFormat::SNIPPET
                } else {
                    InsertTextFormat::PLAIN_TEXT
                }),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(replacement, text))),
                ..Default::default()
            });
        }
    }
    let durations = || ["0s", "30s", "5m", "25m", "1h"].map(String::from).to_vec();
    // What a declared attribute lists to offer comes first.
    let listed = context
        .and_then(|(name, _)| doc.declared_attribute(name.strip_prefix('@')?))
        .map(|declared| declared.values.clone())
        .filter(|values| !values.is_empty());
    let literals: Vec<String> = match context.map(|(name, _)| (name, attribute_value(doc, name))) {
        _ if listed.is_some() => listed.unwrap_or_default(),
        Some((_, Some(AttributeValue::When))) => ["today", "tomorrow", "next Friday"]
            .map(String::from)
            .to_vec(),
        Some((_, Some(AttributeValue::Duration))) => durations(),
        Some((_, None)) if kind.as_ref().and_then(Option::as_deref) == Some("Duration") => {
            durations()
        }
        Some((_, Some(AttributeValue::Date))) => vec![request.today().to_string()],
        _ => Vec::new(),
    };
    for name in literals {
        result.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::VALUE),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(replacement, name))),
            ..Default::default()
        });
    }
    result
}

/// Inside a row expression, the columns of the table it walks.
fn column_completions(
    request: &crate::Request<'_>,
    path: &Path,
    row: usize,
    byte: usize,
    replacement: Range,
) -> Option<Vec<CompletionItem>> {
    let ws = request.workspace();
    let doc = &ws.documents()[path];
    let table_name = lang::eval::tables::scope_at(doc, Span::new(row, byte, byte))?;
    let origin = lang::eval::tables::origin(ws, path, &table_name).ok()?;
    let table = lang::eval::tables::table(ws, &origin)?;
    Some(
        table
            .columns
            .iter()
            .enumerate()
            .map(|(i, column)| CompletionItem {
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
            })
            .collect(),
    )
}

/// After `receiver.`, the receiver's fields: a note's names, a record's keys
/// or a value's properties.
fn property_completions(
    request: &crate::Request<'_>,
    path: &Path,
    before: &str,
    replacement: Range,
) -> Vec<CompletionItem> {
    let ws = request.workspace();
    let mut engine = request.engine();
    let mut result = vec![];
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
                detail: Some(describe::summary(&preview)),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                    replacement,
                    name.clone(),
                ))),
                ..Default::default()
            });
        }
    }
    result
}
