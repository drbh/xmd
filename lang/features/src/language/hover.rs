//! Hovers: what the editor explains about the thing under the cursor.
use crate::locate::Target;
use common::Span;
use eval::engine::{Engine, Value};
use eval::resources::{self, ResourcePresenting};
use eval::{Symbol, SymbolKind, Workspace};
use lsp_types::*;
use std::path::Path;

pub(crate) fn markup(value: String) -> MarkupContent {
    MarkupContent {
        kind: MarkupKind::Markdown,
        value,
    }
}

/// The editor's own hover, for whatever `locate` finds at the position:
/// a link, a table cell, a symbol (with a property preview when a reference
/// reads one), a bracketed calculation, or the task on this row.
pub(crate) fn hover_at(
    request: &eval::RequestContext<'_>,
    path: &Path,
    position: Position,
) -> Option<Hover> {
    let ws = request.workspace();
    let doc = ws.documents.get(path)?;
    match crate::locate::target(ws, path, position)? {
        Target::Link(link) => link_hover(request, path, link),
        Target::Cell { table, row, column } => cell_hover(request, path, table, row, column),
        Target::Calculation(calculation) => calculation_hover(request, path, calculation),
        Target::Task(task) => task_hover(request, path, task),
        Target::Symbol(symbol, span) => {
            let mut value = symbol_hover(request, &symbol);
            let mut range = span.range(&doc.text);
            if let Some(reference) = doc
                .references
                .iter()
                .find(|r| r.span == span && r.property.is_some())
            {
                let preview = request
                    .engine()
                    .eval(path, &reference.expression())
                    .map(|v| v.display())
                    .unwrap_or_else(|e| e.to_string());
                value = format!("{} = {preview}\n\n{value}", reference.expression());
                range = Span::new(span.line, span.start, reference.end()).range(&doc.text);
            }
            Some(Hover {
                contents: HoverContents::Markup(markup(value)),
                range: Some(range),
            })
        }
    }
}

/// A task's state, blockers, estimate, timer and subtask progress, worded by
/// the stdlib's `task` module.
fn task_hover(request: &eval::RequestContext<'_>, path: &Path, index: usize) -> Option<Hover> {
    let ws = request.workspace();
    let doc = ws.documents.get(path)?;
    let task = &doc.tasks[index];
    let mut engine = request.engine();
    let (blocked, blocked_error) = match engine.blocked(path, index) {
        Ok(names) => (
            names
                .into_iter()
                .map(|name| {
                    Value::Text(
                        ws.resolve(path, &name)
                            .map(|s| source_link(ws, &s))
                            .unwrap_or(name),
                    )
                })
                .collect(),
            Value::Null,
        ),
        Err(e) => (vec![], Value::Text(e.to_string())),
    };
    let done = engine.task_done(path, index);
    let mut attribute = |key: &str| {
        task.attributes
            .get(key)
            .and_then(|attr| engine.eval(path, &attr.value).ok())
    };
    let estimate = attribute("estimate");
    let timer = attribute("timer");
    let children: Vec<_> = (0..doc.tasks.len())
        .filter(|j| doc.tasks[*j].parent == Some(index))
        .collect();
    let children_done = children
        .iter()
        .filter(|j| engine.task_done(path, **j))
        .count();
    let text = |value: Option<&Value>| value.map_or(Value::Null, |v| Value::Text(v.display()));
    let record = eval::modules::record([
        ("title".into(), Value::Text(task.title.clone())),
        ("done".into(), Value::Bool(done)),
        ("blocked".into(), Value::List(blocked)),
        ("blocked_error".into(), blocked_error),
        ("estimate".into(), text(estimate.as_ref())),
        ("timer".into(), text(timer.as_ref())),
        (
            "countdown".into(),
            match &timer {
                Some(Value::Timer(timer)) => timer.record(),
                _ => Value::Null,
            },
        ),
        ("children".into(), Value::Count(children.len())),
        ("children_done".into(), Value::Count(children_done)),
    ]);
    Some(Hover {
        contents: HoverContents::Markup(markup(engine.present("task", "hover", vec![record]))),
        range: Some(Span::new(task.line, 0, doc.line(task.line).len()).range(&doc.text)),
    })
}

/// `format.series` over a column or a sum's rows: a sparkline and its range,
/// or nothing when fewer than two values can be charted.
fn series(engine: &mut Engine<'_>, values: Vec<Value>) -> Option<String> {
    match engine.call_module("format", "series", vec![Value::List(values)]) {
        Ok(Value::Null) => None,
        Ok(chart) => Some(chart.display()),
        Err(e) => Some(e.to_string()),
    }
}

fn link_hover(request: &eval::RequestContext<'_>, path: &Path, index: usize) -> Option<Hover> {
    let ws = request.workspace();
    let doc = ws.documents.get(path)?;
    let link = &doc.links[index];
    let resource = resources::Resource {
        target: link.target.clone(),
        origin: None,
    };
    Some(Hover {
        contents: HoverContents::Markup(markup(
            resource.presentation(&mut request.engine(), path).hover,
        )),
        range: Some(link.span.range(&doc.text)),
    })
}

pub(crate) fn source_link(ws: &Workspace, symbol: &Symbol) -> String {
    let named = ws.named(symbol);
    let mut uri = common::file_url(&symbol.path).unwrap();
    uri.set_fragment(Some(&format!("L{}", named.span.line + 1)));
    format!("[{}](<{uri}>)", named.name)
}

/// Everything known about one definition, column or decision variable.
pub(crate) fn symbol_hover(request: &eval::RequestContext<'_>, symbol: &Symbol) -> String {
    let ws = request.workspace();
    let now = request.now();
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
                    .map(|v| Value::from(v.clone()).display())
                    .unwrap_or_else(|e| e.clone())
            })
            .collect::<Vec<_>>()
            .join(", ");
        let values: Vec<Value> = table
            .rows
            .iter()
            .filter_map(|r| r.get(c))
            .filter_map(|cell| cell.value.clone().ok().map(Value::from))
            .collect();
        if let Some(domain) = table.domains[c] {
            return format!(
                "**{} · {}**\n\nDecision column of `{name}` ({}): a plan that sums over it chooses {} for every row. Written cell values are notes; the plan's inlays show the choice.\n\nDefinition: {}",
                named.name,
                domain.value_type(),
                match domain {
                    eval::tables::Domain::Choice => "name?",
                    eval::tables::Domain::Count => "name#",
                },
                match domain {
                    eval::tables::Domain::Choice => "yes or no",
                    eval::tables::Domain::Count => "a whole number",
                },
                source_link(ws, symbol)
            );
        }
        let chart = series(&mut engine, values)
            .map(|chart| format!("\n\n{chart}"))
            .unwrap_or_default();
        return format!(
            "**{} · {}**\n\nColumn of `{name}` · {} rows{chart}\n\nValues: {samples}\n\nDefinition: {}",
            named.name,
            table.types[c].map(|t| t.as_str()).unwrap_or("Unknown"),
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
        let plan = symbol.sibling(SymbolKind::Definition(
            ws.documents[&symbol.path].plans[p].definition,
        ));
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
            if let Some(body) = eval::plans::seek_body(&def.source) {
                let vars = [named.name.clone()].into_iter().collect();
                if let Ok((lhs, op, rhs)) =
                    engine.constraint(&symbol.path, body, def.value_span, &vars)
                    && let Ok(difference) = lhs.minus(&rhs)
                {
                    let coefficient = difference.terms.get(&named.name).copied().unwrap_or(0.0);
                    let summary = engine.present(
                        "plan",
                        "seek_summary",
                        vec![
                            Value::Text(op.as_str().into()),
                            Value::Bool(coefficient > 0.0),
                        ],
                    );
                    out.push_str(&format!("\n\nGoal seek: {summary} `{body}`."));
                }
            }
            if let Ok(Value::Plan(plan)) = &value
                && let Ok(text) = ws.modules.call("plan", "hover", vec![plan.record(ws)], now)
            {
                out.push_str(&text.display());
            }
            if let Some(contributions) = engine.sum_contributions(&symbol.path, &def.source) {
                out.push_str("\n\nRow contributions:\n");
                if let Some(chart) = series(&mut engine, contributions.clone()) {
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
            for member in doc
                .members
                .iter()
                .filter(|m| def.value_span.contains(&doc.text, m.span))
            {
                if let Some(input) = eval::member_symbol(ws, &symbol.path, &member.source) {
                    inputs.insert(source_link(ws, &input));
                }
            }
            for reference in doc
                .references
                .iter()
                .filter(|r| def.value_span.contains(&doc.text, r.span))
            {
                if let Ok(input) = eval::tables::resolve_reference(ws, &symbol.path, reference) {
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
    if !engine.wanted().is_empty() {
        let mut keys = engine.wanted().to_vec();
        keys.sort();
        keys.dedup();
        let now = now.to_utc();
        out.push_str("\n\nLookups:");
        for key in keys {
            match key.lookup(&ws.lookups) {
                Some(lookup) => {
                    let age = engine.present(
                        "format",
                        "age",
                        vec![Value::Duration((now - lookup.fetched_at).num_seconds())],
                    );
                    out.push_str(&format!(
                        "\n- {} · {age} · {}",
                        key.describe(),
                        lookup.source
                    ))
                }
                None => out.push_str(&format!("\n- {} · not fetched yet", key.describe())),
            }
        }
    }
    // A host object adds what only it knows: a link's presentation, a timer's
    // state, a checklist's progress.
    if let Ok(value) = &value
        && let Some(object) = value.host()
        && let Some(detail) = object.hover(&mut engine, &symbol.path)
    {
        out.push_str(&detail);
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
fn calculation_hover(
    request: &eval::RequestContext<'_>,
    path: &Path,
    index: usize,
) -> Option<Hover> {
    let ws = request.workspace();
    let doc = ws.documents.get(path)?;
    let calculation = &doc.calculations[index];
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
fn cell_hover(
    request: &eval::RequestContext<'_>,
    path: &Path,
    table: usize,
    row: usize,
    column: usize,
) -> Option<Hover> {
    let doc = request.workspace().documents.get(path)?;
    let t = &doc.tables[table];
    let cell = &t.rows[row][column];
    let value = match &cell.expression {
        Some((inner, _)) => request
            .engine()
            .eval(path, inner)
            .map(|v| (v, Some(inner.clone()))),
        None => cell
            .value
            .clone()
            .map(|v| (Value::from(v), None))
            .map_err(eval::EvalError::Message),
    };
    let text = match value {
        Ok((value, expression)) => format!(
            "**{}.{} · {}**\n\nRow {}: {}{}",
            doc.definitions[t.definition].named.name,
            t.columns[column].name,
            value.type_name(),
            row + 1,
            value.display(),
            expression
                .map(|e| format!("\n\nCalculated from `{e}`"))
                .unwrap_or_default()
        ),
        Err(error) => error.to_string(),
    };
    Some(Hover {
        contents: HoverContents::Markup(markup(text)),
        range: Some(cell.span.range(&doc.text)),
    })
}
