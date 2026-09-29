//! Hovers: what the editor explains about the thing under the cursor.
use crate::locate;
use crate::locate::Target;
use lang::common::Span;
use lang::eval::engine::{Engine, HostPresenting, Value};
use lang::eval::plans::PlanValue;
use lang::eval::resources::{self, ResourcePresenting};
use lang::eval::timers::Timer;
use lang::eval::{Symbol, SymbolKind, Workspace};
use lsp_types::*;
use std::path::Path;

pub fn markup(value: String) -> MarkupContent {
    MarkupContent {
        kind: MarkupKind::Markdown,
        value,
    }
}

/// The editor's own hover, for whatever `locate` finds at the position:
/// a link, a table cell, a symbol (with a property preview when a reference
/// reads one), a bracketed calculation, or the task on this row.
pub fn hover_at(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    position: Position,
) -> Option<Hover> {
    let ws = request.workspace();
    match locate::target(ws, path, position)? {
        Target::Link(link) => link_hover(request, path, link),
        Target::Cell { table, row, column } => cell_hover(request, path, table, row, column),
        Target::Calculation(calculation) => calculation_hover(request, path, calculation),
        Target::Task(task) => task_hover(request, path, task),
        Target::Symbol(symbol, span) => Some(reference_hover(request, path, &symbol, span)),
    }
}

/// A symbol's hover; a reference that reads a property previews it first.
fn reference_hover(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    symbol: &Symbol,
    span: Span,
) -> Hover {
    let doc = &request.workspace().documents()[path];
    let mut value = symbol_hover(request, symbol);
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
    Hover {
        contents: HoverContents::Markup(markup(value)),
        range: Some(range),
    }
}

/// A task's state, blockers, estimate, timer and subtask progress, worded by
/// the stdlib's `task` module.
fn task_hover(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    index: usize,
) -> Option<Hover> {
    let ws = request.workspace();
    let doc = ws.documents().get(path)?;
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
    let children: Vec<_> = doc
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.parent == Some(index))
        .map(|(j, _)| j)
        .collect();
    let children_done = children
        .iter()
        .filter(|j| engine.task_done(path, **j))
        .count();
    let text = |value: Option<&Value>| value.map_or(Value::Null, |v| Value::Text(v.display()));
    let record = lang::eval::modules::record([
        ("title", Value::Text(task.title.clone())),
        ("done", Value::Bool(done)),
        (
            "in_progress",
            Value::Bool(engine.task_in_progress(path, index)),
        ),
        ("blocked", Value::List(blocked)),
        ("blocked_error", blocked_error),
        ("estimate", text(estimate.as_ref())),
        ("timer", text(timer.as_ref())),
        (
            "countdown",
            match timer.as_ref().and_then(Value::downcast::<Timer>) {
                Some(timer) => timer.record(),
                None => Value::Null,
            },
        ),
        ("children", Value::Count(children.len())),
        ("children_done", Value::Count(children_done)),
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

fn link_hover(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    index: usize,
) -> Option<Hover> {
    let ws = request.workspace();
    let doc = ws.documents().get(path)?;
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

pub fn source_link(ws: &Workspace, symbol: &Symbol) -> String {
    let named = ws.named(symbol);
    let mut uri = lang::common::uri(&symbol.path);
    uri.set_fragment(Some(&format!("L{}", named.span.line + 1)));
    format!("[{}](<{uri}>)", named.name)
}

/// Everything known about one definition, column or decision variable.
pub fn symbol_hover(request: &lang::eval::RequestContext<'_>, symbol: &Symbol) -> String {
    let ws = request.workspace();
    let mut engine = request.engine();
    if let SymbolKind::Column(t, c) = symbol.kind {
        return column_hover(ws, &mut engine, symbol, t, c);
    }
    let named = ws.named(symbol);
    let value = engine.symbol(symbol);
    let mut out = match &value {
        Ok(v) => format!("**{} · {}**\n\n{}", named.name, v.type_name(), v.display()),
        Err(e) => format!("**{}**\n\n{e}", named.name),
    };
    if let SymbolKind::Variable(p, _) = symbol.kind {
        let plan = symbol.sibling(SymbolKind::Definition(
            ws.documents()[&symbol.path].plans[p].definition,
        ));
        out.push_str(&format!(
            "\n\nDecision variable of {}: no note defines this name, so the plan chooses its value.",
            source_link(ws, &plan)
        ));
    }
    if let SymbolKind::Definition(i) = symbol.kind {
        let def = &ws.documents()[&symbol.path].definitions[i];
        if def.expression && def.source != "table" {
            out.push_str(&expression_detail(request, &mut engine, symbol, i, &value));
        }
    }
    out.push_str(&lookups(ws, &mut engine, request.now()).unwrap_or_default());
    // A host object adds what only it knows: a link's presentation, a timer's
    // state, a checklist's progress.
    if let Ok(value) = &value
        && let Some(detail) = value.host_hover(&mut engine, &symbol.path)
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

/// A table column: its decision domain, or its type, rows, chart and samples.
fn column_hover(
    ws: &Workspace,
    engine: &mut Engine<'_>,
    symbol: &Symbol,
    t: usize,
    c: usize,
) -> String {
    let named = ws.named(symbol);
    let doc = &ws.documents()[&symbol.path];
    let table = &doc.tables[t];
    let name = &doc.definitions[table.definition].named.name;
    if let Some(domain) = table.domains[c] {
        return format!(
            "**{} · {}**\n\nDecision column of `{name}` ({}): a plan that sums over it chooses {} for every row. Written cell values are notes; the plan's inlays show the choice.\n\nDefinition: {}",
            named.name,
            domain.value_type(),
            match domain {
                lang::eval::tables::Domain::Choice => "name?",
                lang::eval::tables::Domain::Count => "name#",
            },
            match domain {
                lang::eval::tables::Domain::Choice => "yes or no",
                lang::eval::tables::Domain::Count => "a whole number",
            },
            source_link(ws, symbol)
        );
    }
    let cells: Vec<_> = table.rows.iter().filter_map(|r| r.get(c)).collect();
    let samples = cells
        .iter()
        .take(8)
        .map(|cell| {
            cell.value
                .as_ref()
                .map(|v| Value::from(v.clone()).display())
                .unwrap_or_else(|e| e.clone())
        })
        .collect::<Vec<_>>()
        .join(", ");
    let values = cells
        .iter()
        .filter_map(|cell| cell.value.clone().ok().map(Value::from))
        .collect();
    let chart = series(engine, values)
        .map(|chart| format!("\n\n{chart}"))
        .unwrap_or_default();
    format!(
        "**{} · {}**\n\nColumn of `{name}` · {} rows{chart}\n\nValues: {samples}\n\nDefinition: {}",
        named.name,
        table.types[c].map(|t| t.as_str()).unwrap_or("Unknown"),
        table.rows.len(),
        source_link(ws, symbol)
    )
}

/// An expression definition worked through, then what it seeks, plans, sums
/// and reads.
fn expression_detail(
    request: &lang::eval::RequestContext<'_>,
    engine: &mut Engine<'_>,
    symbol: &Symbol,
    definition: usize,
    value: &lang::eval::EvalResult<Value>,
) -> String {
    let ws = request.workspace();
    let def = &ws.documents()[&symbol.path].definitions[definition];
    let substituted = engine
        .substituted(&symbol.path, &def.source)
        .unwrap_or_else(|_| def.source.clone());
    let mut out = worked(
        &def.source,
        Some(substituted.as_str()).filter(|s| *s != def.source),
        value.as_ref().ok(),
    );
    out.push_str(&goal_seek(ws, engine, symbol, definition).unwrap_or_default());
    if let Ok(value) = value
        && let Some(plan) = value.downcast::<PlanValue>()
        && let Ok(text) = ws
            .modules()
            .call("plan", "hover", vec![plan.record(ws)], request.now())
    {
        out.push_str(&text.display());
    }
    out.push_str(&contributions(engine, &symbol.path, &def.source).unwrap_or_default());
    let inputs: std::collections::BTreeSet<_> =
        locate::reads_within(ws, &symbol.path, def.value_span)
            .map(|(input, _)| source_link(ws, &input))
            .collect();
    if !inputs.is_empty() {
        out.push_str(&format!(
            "\n\nInputs: {}",
            inputs.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    out
}

/// A calculation's source, then its substitution and value where known.
fn worked(source: &str, substituted: Option<&str>, value: Option<&Value>) -> String {
    let mut out = format!("\n\n```text\n{}\n", source.replace('`', "\\`"));
    if let Some(substituted) = substituted {
        out.push_str(&format!("= {substituted}\n"));
    }
    if let Some(v) = value {
        out.push_str(&format!("= {}\n", v.display()));
    }
    out.push_str("```");
    out
}

/// Which way a goal-seeking definition moves its own name.
fn goal_seek(
    ws: &Workspace,
    engine: &mut Engine<'_>,
    symbol: &Symbol,
    definition: usize,
) -> Option<String> {
    let def = &ws.documents()[&symbol.path].definitions[definition];
    let body = lang::eval::plans::seek_body(&def.source)?;
    let name = &def.named.name;
    let vars = [name.clone()].into_iter().collect();
    let (lhs, op, rhs) = engine
        .constraint(&symbol.path, body, def.value_span, &vars)
        .ok()?;
    let difference = lhs.minus(&rhs).ok()?;
    let coefficient = difference.terms.get(name).copied().unwrap_or(0.0);
    let summary = engine.present(
        "plan",
        "seek_summary",
        vec![
            Value::Text(op.as_str().into()),
            Value::Bool(coefficient > 0.0),
        ],
    );
    Some(format!("\n\nGoal seek: {summary} `{body}`."))
}

/// What each row adds to a sum over a table, charted, the first 30 listed.
fn contributions(engine: &mut Engine<'_>, path: &Path, source: &str) -> Option<String> {
    let contributions = engine.sum_contributions(path, source)?;
    let mut out = String::from("\n\nRow contributions:\n");
    if let Some(chart) = series(engine, contributions.clone()) {
        out.push_str(&format!("\n{chart}\n"));
    }
    for (row, value) in contributions.iter().enumerate().take(30) {
        out.push_str(&format!("\n- Row {}: {}", row + 1, value.display()));
    }
    if contributions.len() > 30 {
        out.push_str(&format!("\n- … {} more rows", contributions.len() - 30));
    }
    Some(out)
}

/// The cached lookups the evaluation wanted, with each one's age and source.
fn lookups(
    ws: &Workspace,
    engine: &mut Engine<'_>,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Option<String> {
    if engine.wanted().is_empty() {
        return None;
    }
    let mut keys = engine.wanted().to_vec();
    keys.sort();
    keys.dedup();
    let now = now.to_utc();
    let mut out = String::from("\n\nLookups:");
    for key in keys {
        match key.lookup(ws.lookups()) {
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
    Some(out)
}

/// A bracketed calculation in prose: its expression, substitution and value.
fn calculation_hover(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    index: usize,
) -> Option<Hover> {
    let ws = request.workspace();
    let doc = ws.documents().get(path)?;
    let calculation = &doc.calculations[index];
    let mut engine = request.engine();
    let value = engine.eval_at(path, &calculation.source, calculation.span);
    let mut text = match &value {
        Ok(v) => format!("**{} · {}**", v.display(), v.type_name()),
        Err(e) => format!("**Calculation**\n\n{e}"),
    };
    // Line calculations keep spaces where their brackets were; show them tidy.
    let tidy = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let substituted = engine
        .substituted(path, &calculation.source)
        .ok()
        .filter(|s| *s != calculation.source)
        .map(|s| tidy(&s));
    text.push_str(&worked(
        &tidy(&calculation.source),
        substituted.as_deref(),
        value.as_ref().ok(),
    ));
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
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    table: usize,
    row: usize,
    column: usize,
) -> Option<Hover> {
    let doc = request.workspace().documents().get(path)?;
    let t = &doc.tables[table];
    let cell = &t.rows[row][column];
    let value = match &cell.expression {
        Some((inner, _)) => request
            .engine()
            .eval(path, inner)
            .map(|v| (v, Some(inner.clone()))),
        None => lang::eval::tables::literal_value(cell).map(|v| (v, None)),
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
