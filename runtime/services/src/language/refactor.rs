use crate::controls::code_actions;
use lang::common::Span;
use lang::eval::engine::{Engine, Value, literal};
use lang::eval::{Symbol, SymbolKind, Workspace};
use lang::model::{byte_at, expression_regions, identifier};
use lsp_types::*;
use std::path::Path;

pub(crate) struct Refactor {
    pub title: String,
    pub kind: CodeActionKind,
    pub edits: Vec<TextEdit>,
}
fn unique(ws: &Workspace, path: &Path, stem: &str) -> String {
    (0..)
        .map(|n| {
            if n == 0 {
                stem.into()
            } else {
                format!("{stem}_{n}")
            }
        })
        .find(|n| {
            !ws.symbols()
                .iter()
                .any(|s| s.path == path && ws.named(s).name == *n)
        })
        .unwrap()
}
fn distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut d = vec![vec![0; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, value) in d[0].iter_mut().enumerate() {
        *value = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}
pub(crate) fn refactors(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    range: Range,
) -> Vec<Refactor> {
    let ws = request.workspace();

    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    if range.start.line != range.end.line || crate::locate::inert(doc, range.start) {
        return vec![];
    }
    let row = range.start.line as usize;
    let mut result = vec![];
    // A plan line offers to write its decision-column choices into the note.
    for plan in doc.plans.iter().filter(|p| {
        p.definition < doc.definitions.len() && doc.definitions[p.definition].named.span.line == row
    }) {
        let symbol = Symbol::new(path, SymbolKind::Definition(plan.definition));
        if let Ok(Value::Plan(solved)) = request.engine().symbol(&symbol) {
            let edits: Vec<TextEdit> = ws
                .modules()
                .call(
                    "plan",
                    "write_edits",
                    vec![
                        solved.record(ws),
                        Value::Text(lang::common::file_url(path).unwrap().to_string()),
                    ],
                    request.now(),
                )
                .and_then(|v| lang::eval::modules::json(&v))
                .map_err(|e| e.to_string())
                .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
                .unwrap_or_default();
            if !edits.is_empty() {
                result.push(Refactor {
                    title: ws
                        .modules()
                        .call("plan", "write_title", vec![], request.now())
                        .map(|v| v.display())
                        .unwrap_or_default(),
                    kind: CodeActionKind::REFACTOR_REWRITE,
                    edits,
                });
            }
        }
    }
    // Moving row-local expressions out of their sum, or treating literal table
    // cells as prose, would change their meaning. Do not offer those refactors.
    if doc
        .tables
        .iter()
        .any(|t| row >= t.header && row < t.end_line)
        || doc
            .plans
            .iter()
            .any(|p| row >= p.header && row < p.end_line)
        || doc
            .references
            .iter()
            .any(|r| r.span.line == row && lang::eval::tables::scope_at(doc, r.span).is_some())
    {
        return result;
    }
    let line = doc.line(row);
    let Some(start) = byte_at(line, range.start.character) else {
        return vec![];
    };
    let Some(end) = byte_at(line, range.end.character) else {
        return vec![];
    };
    if start > end {
        return vec![];
    }
    let newline = if doc.text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let regions = expression_regions(doc);
    if start != end {
        let selected = &line[start..end];
        if let Some(region) = regions
            .iter()
            .find(|s| s.contains(&doc.text, Span::new(row, start, end)))
        {
            if Engine::is_subexpression(
                region.source(&doc.text),
                region
                    .offset_of(&doc.text, Span::new(row, start, end))
                    .unwrap(),
                region
                    .offset_of(&doc.text, Span::new(row, start, end))
                    .unwrap()
                    + end
                    - start,
            ) && !identifier(selected)
            {
                let name = unique(ws, path, "calculation");
                result.push(Refactor {
                    title: format!("Extract named calculation '{name}'"),
                    kind: CodeActionKind::REFACTOR_EXTRACT,
                    edits: vec![
                        TextEdit::new(
                            Range::new(Position::new(row as u32, 0), Position::new(row as u32, 0)),
                            format!("[{name}] := {selected}{newline}"),
                        ),
                        TextEdit::new(range, name),
                    ],
                });
            }
        } else if !doc.definitions.iter().any(|d| {
            d.named.span.line == row
                && start < d.end.end
                && end > d.value_span.start.saturating_sub(1)
        }) && !doc
            .references
            .iter()
            .any(|r| r.span.line == row && start <= r.end() && end > r.span.start.saturating_sub(1))
            && !doc
                .tasks
                .iter()
                .flat_map(|t| t.attributes.values())
                .chain(doc.events.iter().flat_map(|e| e.attributes.values()))
                .any(|a| {
                    a.value_span.line == row && start < a.value_span.end && end > a.value_span.start
                })
            && let Ok(value) = literal(selected)
            && !matches!(
                value,
                Value::Text(_) | Value::Tasks(_) | Value::Timer(_) | Value::Resource(_)
            )
            && !selected.contains(['[', ']', '\n', '\r'])
        {
            let name = unique(
                ws,
                path,
                if matches!(value, Value::Money(..)) {
                    "amount"
                } else {
                    "value"
                },
            );
            result.push(Refactor {
                title: format!("Extract named value '{name}'"),
                kind: CodeActionKind::REFACTOR_EXTRACT,
                edits: vec![TextEdit::new(range, format!("[{selected}]:{name}"))],
            });
        }
    }
    for reference in doc.references.iter().filter(|r| r.span.line == row) {
        let begin = if reference.bracket {
            line[..reference.span.start]
                .rfind('[')
                .unwrap_or(reference.span.start)
        } else {
            reference.span.start
        };
        let finish = if reference.bracket {
            reference.end() + line[reference.end()..].find(']').unwrap_or(0) + 1
        } else {
            let mut finish = reference.end();
            while line[finish..].starts_with('.') {
                let property = line[finish + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .map(char::len_utf8)
                    .sum::<usize>();
                if property == 0 {
                    break;
                }
                finish += property + 1;
            }
            finish
        };
        if start > finish || end < begin || (start != end && (start < begin || end > finish)) {
            continue;
        }
        if let Err(error) = ws.resolve(path, &reference.name) {
            if !matches!(error, lang::eval::EvalError::UnknownName { .. }) {
                continue;
            }
            let mut choices: Vec<_> = ws
                .symbols()
                .into_iter()
                .filter_map(|s| {
                    let n = &ws.named(&s).name;
                    let dist = distance(&reference.name, n);
                    (dist <= 2 && ws.resolve(path, n).ok().as_ref() == Some(&s))
                        .then_some((dist, n.clone()))
                })
                .collect();
            choices.sort();
            choices.dedup();
            for (_, name) in choices.into_iter().take(3) {
                result.push(Refactor {
                    title: format!("Change '{}' to '{name}'", reference.name),
                    kind: CodeActionKind::QUICKFIX,
                    edits: vec![TextEdit::new(reference.span.range(&doc.text), name)],
                });
            }
            result.push(Refactor {
                title: format!("Create definition '{}' (TODO placeholder)", reference.name),
                kind: CodeActionKind::QUICKFIX,
                edits: vec![TextEdit::new(
                    Range::new(Position::new(row as u32, 0), Position::new(row as u32, 0)),
                    format!("[\"TODO\"]:{}{newline}", reference.name),
                )],
            });
            continue;
        }
        let symbol = ws.resolve(path, &reference.name).unwrap();
        let replace = Span::new(row, begin, finish).range(&doc.text);
        let mut engine = request.engine();
        let expression = if reference.bracket {
            reference.expression()
        } else {
            line[begin..finish].into()
        };
        if let Ok(value) = engine.eval(path, &expression)
            && let Some(source) = if reference.bracket {
                (!matches!(
                    value,
                    Value::Timer(_)
                        | Value::Resource(_)
                        | Value::Tasks(_)
                        | Value::Table(_)
                        | Value::Plan(_)
                ) && !value.display().contains(['[', ']', '\n', '\r']))
                .then(|| value.display())
            } else {
                value
                    .source()
                    .filter(|s| engine.eval(path, s).ok().as_ref() == Some(&value))
            }
        {
            result.push(Refactor {
                title: "Freeze current value".into(),
                kind: CodeActionKind::REFACTOR_REWRITE,
                edits: vec![TextEdit::new(
                    replace,
                    if reference.bracket {
                        source
                    } else {
                        format!("({source})")
                    },
                )],
            });
        }
        if reference.bracket {
            continue;
        }
        if let SymbolKind::Definition(index) = symbol.kind {
            let original = &ws.documents()[&symbol.path];
            let def = &original.definitions[index];
            if !def.expression
                || !Engine::valid_expression(&def.source)
                || matches!(
                    engine.named(path, &reference.name),
                    Ok(Value::Timer(_) | Value::Resource(_) | Value::Table(_) | Value::Plan(_))
                )
            {
                continue;
            }
            let safe = original
                .references
                .iter()
                .filter(|r| def.value_span.contains(&original.text, r.span))
                .all(|r| ws.resolve(&symbol.path, &r.name).ok() == ws.resolve(path, &r.name).ok());
            if safe {
                result.push(Refactor {
                    title: "Inline expression".into(),
                    kind: CodeActionKind::REFACTOR_INLINE,
                    edits: vec![TextEdit::new(
                        reference.span.range(&doc.text),
                        format!("({})", def.source),
                    )],
                });
            }
        }
    }
    // Guard every offered transformation against invalid ranges/overlapping edits.
    result.retain(|r| code_actions::apply_edits(&doc.text, &r.edits).is_ok());
    result
}
