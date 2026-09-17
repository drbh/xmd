use crate::{
    actions,
    document::{Document, Span, byte_at, identifier},
    engine::{Engine, Value, literal},
    intelligence,
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset};
use lsp_types::*;
use std::path::Path;

pub struct Refactor {
    pub title: String,
    pub kind: CodeActionKind,
    pub edits: Vec<TextEdit>,
}
fn unique(ws: &Workspace, stem: &str) -> String {
    (0..)
        .map(|n| {
            if n == 0 {
                stem.into()
            } else {
                format!("{stem}_{n}")
            }
        })
        .find(|n| !ws.symbols().iter().any(|s| ws.named(s).name == *n))
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
pub fn expression_regions(doc: &Document) -> Vec<Span> {
    doc.definitions
        .iter()
        .filter(|d| d.expression)
        .map(|d| d.value_span)
        .chain(
            doc.plans
                .iter()
                .flat_map(|p| p.constraints.iter().map(|c| c.span)),
        )
        .chain(doc.tasks.iter().flat_map(|t| {
            t.attributes
                .iter()
                .filter(|(k, _)| {
                    matches!(
                        k.as_str(),
                        "due" | "scheduled" | "at" | "after" | "estimate"
                    )
                })
                .map(|(_, a)| a.value_span)
        }))
        .collect()
}
pub fn actions_for(
    ws: &Workspace,
    path: &Path,
    range: Range,
    now: DateTime<FixedOffset>,
) -> Vec<Refactor> {
    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    if range.start.line != range.end.line || intelligence::inert(doc, range.start) {
        return vec![];
    }
    let row = range.start.line as usize;
    let mut result = vec![];
    // A plan line offers to write its decision-column choices into the note.
    for plan in doc.plans.iter().filter(|p| {
        p.definition < doc.definitions.len() && doc.definitions[p.definition].named.span.line == row
    }) {
        let symbol = Symbol {
            path: path.into(),
            kind: SymbolKind::Definition(plan.definition),
        };
        if let Ok(Value::Plan(solved)) = Engine::at(ws, now).symbol(&symbol) {
            let edits: Vec<TextEdit> = solved
                .rows
                .iter()
                .filter(|(r, _)| r.table.path == path)
                .filter_map(|(r, value)| {
                    let cell = crate::tables::table(ws, &r.table)?
                        .rows
                        .get(r.row)?
                        .get(r.column)?;
                    let text = match value {
                        Value::Bool(true) => "yes".to_string(),
                        Value::Bool(false) => "no".to_string(),
                        v => v.display(),
                    };
                    if cell.source == text {
                        return None;
                    }
                    // An empty cell's span sits at the end of its padding; fill the
                    // whole gap between the pipes and keep the column width.
                    let line = doc.line(cell.span.line).as_bytes();
                    let (mut a, mut b) = (cell.span.start, cell.span.end);
                    while a > 0 && line[a - 1] == b' ' {
                        a -= 1;
                    }
                    while b < line.len() && line[b] == b' ' {
                        b += 1;
                    }
                    let width = (b - a).saturating_sub(2).max(text.len());
                    Some(TextEdit::new(
                        Span::new(cell.span.line, a, b).range(&doc.text),
                        format!(" {text:<width$} "),
                    ))
                })
                .collect();
            if !edits.is_empty() {
                result.push(Refactor {
                    title: "Write the plan's choices into the table".into(),
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
            .any(|r| r.span.line == row && crate::tables::scope_at(doc, r.span).is_some())
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
            .find(|s| s.line == row && start >= s.start && end <= s.end)
        {
            if Engine::is_subexpression(
                &line[region.start..region.end],
                start - region.start,
                end - region.start,
            ) && !identifier(selected)
            {
                let name = unique(ws, "calculation");
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
            if !error.starts_with("Unknown name") {
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
        let mut engine = Engine::at(ws, now);
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
            let original = &ws.documents[&symbol.path];
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
                .filter(|r| {
                    r.span.line == def.value_span.line && r.span.start >= def.value_span.start
                })
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
    result.retain(|r| actions::apply_edits(&doc.text, &r.edits).is_ok());
    result
}
