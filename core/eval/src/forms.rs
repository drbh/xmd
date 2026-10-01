//! Evaluating a definition that calls a form a module declares: the host
//! reads every expression the form is handed in the note's scope, as the
//! declaration says (a linear form over the unknowns, a constraint), and the
//! module's `define` hook says what the definition is worth from those
//! values. The module reads values, never note code; what a form means — a
//! plan's solution, a goal seek's boundary — is the module's, in .xmd.
//!
//! The form's unknowns are names of the note ([`Unknowns::Free`]: each reads
//! as the field of that name of the definition's value, through the
//! workspace's own resolution and cycle checks) or the definition's own name
//! ([`Unknowns::Own`], found through the definitions that read it).
//! `features` registers this for [`document::DefinitionKind::Form`].
use crate::engine::{Engine, Linear, RowVariable, Unit, Value};
use crate::linear::Vars;
use crate::workspace::{Symbol, Workspace};
use common::Span;
use document::forms::{Formed, Reading, Unknowns};
use std::path::Path;
use values::{EvalError, EvalResult, geometry, record};

/// Evaluate form definition `i` of `symbol`'s note: read its expressions,
/// then ask its module's `define` hook. What the hook says besides the value
/// (a hover, a detail, a record) is kept with the value: [`Engine::about`].
pub(crate) fn evaluate(engine: &mut Engine<'_>, symbol: &Symbol, i: usize) -> EvalResult<Value> {
    let ws = engine.workspace();
    let doc = &ws.documents[&symbol.path];
    let formed = doc.form_of(i).ok_or(EvalError::Expected("a form"))?;
    let path = symbol.path.as_path();
    if let Some(problem) = formed.problems.first() {
        let message = EvalError::Message(problem.message.clone());
        return Err(engine.fail_at(path, problem.span, message));
    }
    let definition = &doc.definitions()[i];
    let unknowns: Vec<String> = match formed.form.unknowns {
        Unknowns::Free => ws
            .claimed(path, formed)
            .into_iter()
            .map(|(_, named)| named.name.clone())
            .collect(),
        Unknowns::Own => vec![definition.named.name.clone()],
    };
    let vars: Vars = unknowns.iter().cloned().collect();
    // A form read while reading another (a constant that is a form's value)
    // keeps its decision cells to itself.
    let outer = std::mem::take(&mut engine.row_variables);
    let read = read_all(engine, path, formed, &vars);
    let decisions = std::mem::replace(&mut engine.row_variables, outer);
    let (arguments, rows) = read?;
    let unknowns = unknowns.into_iter().map(Value::Text).collect();
    let decisions = decisions.iter().filter_map(|d| decision_record(ws, d));
    let input = record([
        ("form", Value::Text(formed.form.name.clone())),
        ("name", Value::Text(definition.named.name.clone())),
        ("document", Value::Text(uri(path))),
        ("line", Value::Count(definition.named.span.line)),
        ("arguments", Value::list(arguments)),
        ("rows", Value::list(rows)),
        ("unknowns", Value::list(unknowns)),
        ("decisions", Value::list(decisions.collect())),
    ]);
    let first = formed.arguments.first();
    let at = first.map_or(definition.value_span, |(_, span)| *span);
    let (module, define) = (&formed.form.module, modules::Hook::Define.as_ref());
    let answer = engine
        .call_hook(module, define, vec![input], true)
        .map_err(|message| engine.fail_at(path, at, message))?;
    let Value::Record(fields) = &answer else {
        let message = EvalError::from(format!(
            "{}.define must return a record with the definition's value",
            formed.form.module
        ));
        return Err(engine.fail_at(path, at, message));
    };
    let Some(value) = fields.get("value").cloned() else {
        let message = EvalError::from(format!(
            "{}.define must return the definition's value under `value`",
            formed.form.module
        ));
        return Err(engine.fail_at(path, at, message));
    };
    engine.about = Some(About::of(fields));
    Ok(value)
}

/// Every argument, then every row's cells, read as the form says, in order:
/// the first that fails fails them all.
fn read_all(
    engine: &mut Engine<'_>,
    path: &Path,
    formed: &Formed,
    vars: &Vars,
) -> EvalResult<(Vec<Value>, Vec<Value>)> {
    let arguments = formed
        .arguments
        .iter()
        .zip(&formed.form.reads)
        .map(|((source, span), reads)| read(engine, path, source, *span, *reads, vars))
        .collect::<EvalResult<Vec<_>>>()?;
    let rows = formed
        .rows
        .iter()
        .map(|cells| {
            let line = cells.first().map_or(0, |(_, span)| span.line);
            let cells = cells
                .iter()
                .zip(&formed.form.table)
                .map(|((source, span), column)| {
                    read(engine, path, source, *span, column.reads, vars)
                })
                .collect::<EvalResult<Vec<_>>>()?;
            Ok(record([
                ("line", Value::Count(line)),
                ("cells", Value::list(cells)),
            ]))
        })
        .collect::<EvalResult<Vec<_>>>()?;
    Ok((arguments, rows))
}

/// One expression, or a name cell, read as `reads` says: `{text, range,
/// anchor}` and what the reading found.
fn read(
    engine: &mut Engine<'_>,
    path: &Path,
    source: &str,
    span: Span,
    reads: Reading,
    vars: &Vars,
) -> EvalResult<Value> {
    let doc = &engine.workspace().documents[path];
    let range = span.range(doc);
    let anchor = geometry(doc.line_end(range.end.line as usize));
    let mut fields = vec![
        ("text".to_owned(), Value::Text(source.into())),
        ("range".to_owned(), geometry(range)),
        ("anchor".to_owned(), anchor),
    ];
    match reads {
        Reading::Name => {}
        Reading::Linear => {
            let form = engine.linear(path, source, span, vars)?;
            if let Value::Record(form) = form_record(&form) {
                fields.extend(form.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }
        Reading::Constraint => {
            let (lhs, op, rhs) = engine.constraint(path, source, span, vars)?;
            let difference = lhs
                .add(&rhs, -1.0)
                .map_or(Value::Null, |difference| form_record(&difference));
            fields.extend([
                ("op".to_owned(), Value::Text(op.as_str().into())),
                ("lhs".to_owned(), form_record(&lhs)),
                ("rhs".to_owned(), form_record(&rhs)),
                ("difference".to_owned(), difference),
            ]);
        }
    }
    Ok(Value::record(fields.into_iter().collect()))
}

/// One of a unit: what tells a module what a form counts in.
fn unit_value(kind: Unit, currency: Option<common::Currency>) -> Value {
    match kind {
        Unit::Money => Value::Money(1.0, currency.unwrap_or(common::Currency::USD)),
        Unit::Duration => Value::Duration(1),
        _ => Value::Number(1.0),
    }
}

/// `{constant, terms, unit, per}`: a linear form as a module reads it.
fn form_record(form: &Linear) -> Value {
    let terms = form
        .terms
        .iter()
        .map(|(name, n)| (name.clone(), Value::Number(*n)));
    record([
        ("constant", Value::Number(form.constant)),
        ("terms", Value::record(terms.collect())),
        ("unit", unit_value(form.kind, form.currency)),
        (
            "per",
            form.unknown_kind()
                .map_or(Value::Null, |kind| unit_value(kind, form.currency)),
        ),
    ])
}

/// A note's URI, as records name it.
pub(crate) fn uri(path: &Path) -> String {
    common::file_url(path)
        .map(|url| url.to_string())
        .unwrap_or_default()
}

/// One decision cell, an unknown of its own: what it is called, its column's
/// domain, and where its cell is, out to the pipes, for a module to show and
/// rewrite it. `None` when its table is no longer in the workspace.
fn decision_record(ws: &Workspace, decision: &RowVariable) -> Option<Value> {
    let table = crate::tables_impl::table(ws, &decision.table)?;
    let doc = &ws.documents[&decision.table.path];
    let cell = table.rows.get(decision.row)?.get(decision.column)?;
    let line = doc.line(cell.span.line);
    // The cell with its padding, out to the pipes.
    let head = line.get(..cell.span.start).unwrap_or(line);
    let tail = line.get(cell.span.end..).unwrap_or("");
    let a = head.trim_end_matches(' ').len();
    let b = line.len() - tail.trim_start_matches(' ').len();
    let crate::workspace::SymbolKind::Definition(definition) = decision.table.kind else {
        return None;
    };
    let domain: &str = decision.domain.into();
    let label = table.rows[decision.row].first().map(|c| c.source.clone());
    let label = label.unwrap_or_else(|| (decision.row + 1).to_string());
    let padded = Span::new(cell.span.line, a, b);
    let text = |text: &str| Value::Text(text.into());
    Some(record([
        ("name", text(&decision.name)),
        ("domain", text(domain)),
        ("table", text(&doc.definitions()[definition].named.name)),
        ("column", text(&table.columns[decision.column].name)),
        ("row", Value::Count(decision.row)),
        ("label", Value::Text(label)),
        ("document", Value::Text(uri(&decision.table.path))),
        ("source", text(&cell.source)),
        ("line", Value::Count(cell.span.line)),
        ("range", geometry(padded.range(doc))),
        ("anchor", geometry(cell.span.range(doc).end)),
        ("width", Value::Count((b - a).saturating_sub(2))),
    ]))
}

/// What a form's module said about a definition besides its value.
#[derive(Clone, Debug, PartialEq)]
pub struct About {
    /// Markdown its hover adds after the calculation worked through.
    pub hover: Option<String>,
    /// The one line the outline and the call hierarchy show for it.
    pub detail: Option<String>,
    /// What its records' `record` field holds; null when the module says none.
    pub record: Value,
}
impl About {
    /// The hook's answer besides `value`: text where text is shown.
    fn of(answer: &std::collections::BTreeMap<String, Value>) -> Self {
        let text = |name: &str| match answer.get(name) {
            None | Some(Value::Null) => None,
            Some(Value::Text(text)) => Some(text.clone()),
            Some(other) => Some(other.display()),
        };
        Self {
            hover: text("hover"),
            detail: text("detail"),
            record: answer.get("record").cloned().unwrap_or(Value::Null),
        }
    }
}

impl Engine<'_> {
    /// What the module that evaluates form definition `symbol` said about it
    /// besides its value, or `None` for any other definition and one that
    /// fails.
    pub fn about(&mut self, symbol: &Symbol) -> Option<About> {
        self.symbol(symbol).ok()?;
        self.request.memo.about(symbol)
    }
}
