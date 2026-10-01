use crate::locate;
use lang::common::Span;
use lang::eval::engine::{Engine, Value, literal};
use lang::eval::{SymbolKind, Workspace};
use lang::model::{byte_at, expression_regions, identifier};
use lsp_types::*;
use std::path::Path;

/// One proposal, before a host decides how to carry it: an edit, a command, or
/// an edit it must show as unavailable with a reason.
#[derive(Clone, Debug)]
pub struct CodeActionItem {
    pub title: String,
    pub kind: Option<CodeActionKind>,
    pub edits: Vec<TextEdit>,
    pub command: Option<Command>,
    pub disabled: Option<String>,
}
impl CodeActionItem {
    pub fn edit(title: String, kind: CodeActionKind, edits: Vec<TextEdit>) -> Self {
        Self {
            title,
            kind: Some(kind),
            edits,
            command: None,
            disabled: None,
        }
    }
    pub fn command(command: Command) -> Self {
        Self {
            title: command.title.clone(),
            kind: None,
            edits: vec![],
            command: Some(command),
            disabled: None,
        }
    }
}

fn unique(ws: &Workspace, path: &Path, stem: &str) -> String {
    let taken: Vec<String> = ws
        .symbols()
        .iter()
        .filter(|s| s.path == path)
        .map(|s| ws.named(s).name.clone())
        .collect();
    (0..)
        .map(|n| {
            if n == 0 {
                stem.into()
            } else {
                format!("{stem}_{n}")
            }
        })
        .find(|n| !taken.contains(n))
        .unwrap()
}
/// Whether two spans share a character.
fn overlaps(a: Span, b: Span) -> bool {
    a.line == b.line && a.start < b.end && b.start < a.end
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
pub fn refactors(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    range: Range,
) -> Vec<CodeActionItem> {
    let ws = request.workspace();

    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    if range.start.line != range.end.line || locate::inert(doc, range.start) {
        return vec![];
    }
    let row = range.start.line as usize;
    let mut result = vec![];
    // Moving row-local expressions out of their sum, or treating literal table
    // cells as prose, would change their meaning. Do not offer those refactors.
    if doc
        .tables
        .iter()
        .any(|t| row >= t.header && row < t.end_line)
        || doc
            .forms
            .iter()
            .any(|f| f.has_table() && row >= f.header && row < f.end_line)
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
        let selection = Span::new(row, start, end);
        if let Some(region) = regions.iter().find(|s| s.contains(doc, selection)) {
            let offset = region.offset_of(doc, selection).unwrap();
            if Engine::is_subexpression(region.source(doc), offset, offset + end - start)
                && !identifier(selected)
            {
                let name = unique(ws, path, "calculation");
                result.push(CodeActionItem::edit(
                    format!("Extract named calculation '{name}'"),
                    CodeActionKind::REFACTOR_EXTRACT,
                    vec![
                        TextEdit::new(
                            Range::new(Position::new(row as u32, 0), Position::new(row as u32, 0)),
                            format!("[{name}] := {selected}{newline}"),
                        ),
                        TextEdit::new(range, name),
                    ],
                ));
            }
        // Definitions and references count the brackets around them.
        } else if !doc.definitions.iter().any(|d| {
            let opening = d.value_span.start.saturating_sub(1);
            overlaps(selection, Span::new(d.named.span.line, opening, d.end.end))
        }) && !doc.references.iter().any(|r| {
            let opening = r.span.start.saturating_sub(1);
            overlaps(selection, Span::new(r.span.line, opening, r.end() + 1))
        }) && !doc
            .claimed_attributes()
            .any(|(_, a)| overlaps(selection, a.value_span))
            && let Ok(value) = literal(selected)
            && !matches!(
                value,
                Value::Text(_) | Value::Tasks(_) | Value::Host(_) | Value::Resource(_)
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
            result.push(CodeActionItem::edit(
                format!("Extract named value '{name}'"),
                CodeActionKind::REFACTOR_EXTRACT,
                vec![TextEdit::new(range, format!("[{selected}]:{name}"))],
            ));
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
            doc.reference_close(reference)
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
                result.push(CodeActionItem::edit(
                    format!("Change '{}' to '{name}'", reference.name),
                    CodeActionKind::QUICKFIX,
                    vec![TextEdit::new(reference.span.range(doc), name)],
                ));
            }
            result.push(CodeActionItem::edit(
                format!("Create definition '{}' (TODO placeholder)", reference.name),
                CodeActionKind::QUICKFIX,
                vec![TextEdit::new(
                    Range::new(Position::new(row as u32, 0), Position::new(row as u32, 0)),
                    format!("[\"TODO\"]:{}{newline}", reference.name),
                )],
            ));
            continue;
        }
        let symbol = ws.resolve(path, &reference.name).unwrap();
        let replace = Span::new(row, begin, finish).range(doc);
        let mut engine = request.engine();
        let expression = if reference.bracket {
            reference.expression()
        } else {
            line[begin..finish].into()
        };
        if let Ok(value) = engine.eval(path, &expression)
            && let Some(source) = if reference.bracket {
                (!matches!(value, Value::Host(_) | Value::Resource(_) | Value::Tasks(_))
                    && !value.display().contains(['[', ']', '\n', '\r']))
                .then(|| value.display())
            } else {
                value
                    .source()
                    .filter(|s| engine.eval(path, s).ok().as_ref() == Some(&value))
            }
        {
            result.push(CodeActionItem::edit(
                "Freeze current value".into(),
                CodeActionKind::REFACTOR_REWRITE,
                vec![TextEdit::new(
                    replace,
                    if reference.bracket {
                        source
                    } else {
                        format!("({source})")
                    },
                )],
            ));
        }
        if reference.bracket {
            continue;
        }
        if let SymbolKind::Definition(index) = symbol.kind {
            let original = &ws.documents()[&symbol.path];
            let def = &original.definitions[index];
            if !def.expression
                || !lang::syntax::valid_expression(&def.source)
                || matches!(
                    engine.named(path, &reference.name),
                    Ok(Value::Host(_) | Value::Resource(_))
                )
            {
                continue;
            }
            let safe = original
                .references
                .iter()
                .filter(|r| def.value_span.contains(original, r.span))
                .all(|r| ws.resolve(&symbol.path, &r.name).ok() == ws.resolve(path, &r.name).ok());
            if safe {
                result.push(CodeActionItem::edit(
                    "Inline expression".into(),
                    CodeActionKind::REFACTOR_INLINE,
                    vec![TextEdit::new(
                        reference.span.range(doc),
                        format!("({})", def.source),
                    )],
                ));
            }
        }
    }
    // Guard every offered transformation against invalid ranges/overlapping edits.
    result.retain(|r| lang::model::apply_edits(&doc.text, &r.edits).is_ok());
    result
}
