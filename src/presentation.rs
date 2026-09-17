//! Host-independent editor presentation, shared by native LSP and WebAssembly.
use crate::{
    document::{Problem, Span},
    engine::{Engine, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset, NaiveDate};
use lsp_types::*;
use std::path::Path;

pub use crate::highlighting::{TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens};

pub fn document_links(
    workspace: &Workspace,
    path: &Path,
    now: DateTime<FixedOffset>,
) -> Vec<DocumentLink> {
    let Some(doc) = workspace.documents.get(path) else {
        return vec![];
    };
    let mut links = Vec::new();
    let mut add = |span: Span, resource: crate::resources::Resource| {
        if let Ok(url) = resource.url(path) {
            links.push(DocumentLink {
                range: span.range(&doc.text),
                tooltip: Some(format!("Open {url}")),
                target: Some(url),
                data: None,
            });
        }
    };
    for link in &doc.links {
        add(
            link.span,
            crate::resources::Resource {
                target: link.target.clone(),
                origin: None,
            },
        );
    }
    let mut engine = Engine::at(workspace, now);
    for (i, def) in doc.definitions.iter().enumerate() {
        if let Ok(Value::Resource(resource)) = engine.symbol(&Symbol {
            path: path.into(),
            kind: SymbolKind::Definition(i),
        }) {
            add(def.value_span, resource);
        }
    }
    for reference in &doc.references {
        if reference.property.is_none()
            && let Ok(Value::Resource(resource)) = engine.named(path, &reference.name)
        {
            add(reference.span, resource);
        }
    }
    links.sort_by_key(|link| (link.range.start, link.range.end));
    links.dedup_by(|a, b| a.range == b.range && a.target == b.target);
    links
}
pub fn problems(workspace: &Workspace, path: &Path, today: NaiveDate) -> Vec<Problem> {
    crate::diagnostics::problems(workspace, path, today)
}
pub fn hints(workspace: &Workspace, path: &Path, today: NaiveDate, range: Range) -> Vec<InlayHint> {
    let mut engine = Engine::new(workspace, today);
    collect_hints(&mut engine, path, range)
}
pub fn hints_at(
    workspace: &Workspace,
    path: &Path,
    now: DateTime<FixedOffset>,
    range: Range,
) -> Vec<InlayHint> {
    collect_hints(&mut Engine::at(workspace, now), path, range)
}
pub fn live_hints(workspace: &Workspace, path: &Path, now: DateTime<FixedOffset>) -> bool {
    let mut engine = Engine::at(workspace, now);
    collect_hints(
        &mut engine,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, 0)),
    );
    engine.time_dependent
}
/// Countdown labels carry a live gauge; stopwatches have no end to measure against.
fn timer_label(timer: &crate::timers::Timer) -> String {
    let text = timer.display();
    match timer.limit {
        Some(limit) => {
            let gauge = crate::charts::gauge_fraction(timer.elapsed as f64 / limit as f64);
            match text.split_once(' ') {
                Some((icon, rest)) => format!("{icon} {gauge} {rest}"),
                None => format!("{gauge} {text}"),
            }
        }
        None => text,
    }
}
fn collect_hints(engine: &mut Engine<'_>, path: &Path, range: Range) -> Vec<InlayHint> {
    let workspace = engine.workspace;
    let today = engine.today;
    let doc = &workspace.documents[path];
    let mut hints = Vec::new();
    let mut push = |position: Position, label: String, tooltip: String| {
        if position >= range.start && position <= range.end {
            hints.push(InlayHint {
                position,
                label: InlayHintLabel::String(label),
                kind: None,
                text_edits: None,
                tooltip: Some(InlayHintTooltip::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: tooltip,
                })),
                padding_left: Some(true),
                padding_right: None,
                data: None,
            });
        }
    };
    for (i, def) in doc.definitions.iter().enumerate() {
        match engine.symbol(&Symbol {
            path: path.to_path_buf(),
            kind: SymbolKind::Definition(i),
        }) {
            Ok(Value::Resource(resource)) => push(
                def.end.range(&doc.text).start,
                resource.label(&workspace.cache, engine.now.to_utc()),
                resource.hover(path, &workspace.cache),
            ),
            Ok(value) if def.expression => push(
                def.end.range(&doc.text).start,
                match &value {
                    Value::Timer(timer) => format!("= {}", timer_label(timer)),
                    Value::Plan(plan) => std::iter::once(format!("= {}", plan.objective.display()))
                        .chain(
                            plan.variables
                                .iter()
                                .map(|(name, v)| format!("{name} {}", v.display())),
                        )
                        .chain(plan.columns().into_iter().map(|(table, column, cells)| {
                            let t = &workspace.documents[&table.path];
                            let name = crate::tables::table(workspace, &table)
                                .map(|t| t.columns[column].name.clone())
                                .unwrap_or_default();
                            let _ = t;
                            let chosen = cells
                                .iter()
                                .filter(|(_, v)| !matches!(v, Value::Bool(false)))
                                .count();
                            match cells.first().map(|(_, v)| v) {
                                Some(Value::Bool(_)) => {
                                    format!("{name} {chosen} of {}", cells.len())
                                }
                                _ => format!(
                                    "{name} {}",
                                    Value::Number(
                                        cells
                                            .iter()
                                            .filter_map(|(_, v)| crate::charts::magnitude(v))
                                            .sum()
                                    )
                                    .display()
                                ),
                            }
                        }))
                        .collect::<Vec<_>>()
                        .join(" · "),
                    value => format!("= {}", value.display()),
                },
                crate::intelligence::hover(
                    workspace,
                    &Symbol {
                        path: path.into(),
                        kind: SymbolKind::Definition(i),
                    },
                    engine.now,
                ),
            ),
            _ => {}
        }
    }
    // Decision cells of tables in this note, filled by plans in any note.
    for (plan_path, plan_doc) in &workspace.documents {
        for plan in &plan_doc.plans {
            let Ok(Value::Plan(solved)) = engine.symbol(&Symbol {
                path: plan_path.clone(),
                kind: SymbolKind::Definition(plan.definition),
            }) else {
                continue;
            };
            for (row, value) in &solved.rows {
                if row.table.path != path {
                    continue;
                }
                let Some(table) = crate::tables::table(workspace, &row.table) else {
                    continue;
                };
                let Some(cell) = table.rows.get(row.row).and_then(|r| r.get(row.column)) else {
                    continue;
                };
                let shown = match value {
                    Value::Bool(true) => "yes".to_string(),
                    Value::Bool(false) => "no".to_string(),
                    v => v.display(),
                };
                push(
                    cell.span.range(&doc.text).end,
                    format!("→ {shown}"),
                    format!(
                        "Chosen by plan {}. Use the code action on the plan to write choices into the table.",
                        plan_doc.definitions[plan.definition].named.name
                    ),
                );
            }
        }
    }
    for plan in &doc.plans {
        let Ok(Value::Plan(solved)) = engine.symbol(&Symbol {
            path: path.to_path_buf(),
            kind: SymbolKind::Definition(plan.definition),
        }) else {
            continue;
        };
        for (constraint, result) in plan.constraints.iter().zip(&solved.constraints) {
            let usage = match (result.op.as_str(), crate::charts::magnitude(&result.rhs)) {
                ("<=", Some(rhs)) if rhs > 0.0 => crate::charts::magnitude(&result.lhs)
                    .map(|lhs| format!("{} ", crate::charts::gauge_fraction(lhs / rhs)))
                    .unwrap_or_default(),
                _ => String::new(),
            };
            let status = if result.binding {
                "binding".to_string()
            } else {
                format!("slack {}", result.slack.display())
            };
            let symbol = match result.op.as_str() {
                "<=" => "≤",
                ">=" => "≥",
                _ => "=",
            };
            let label = format!(
                "{usage}{} {symbol} {} · {status}",
                result.lhs.display(),
                result.rhs.display()
            );
            push(
                doc.line_end(constraint.span.line),
                label.clone(),
                format!(
                    "**{}**\n\n{label}\n\nA binding constraint limits the objective; slack is the unused room.",
                    constraint.named.name
                ),
            );
        }
    }
    for table in &doc.tables {
        for cell in table.rows.iter().flatten() {
            if let Some((inner, span)) = &cell.expression
                && let Ok(value) = engine.eval_at(path, inner, *span)
            {
                push(
                    cell.span.range(&doc.text).end,
                    value.display(),
                    format!("`{inner}` = {}", value.display()),
                );
            }
        }
    }
    let dates = crate::itinerary::dates(&doc.days, today);
    for (day, date) in doc.days.iter().zip(&dates) {
        let Some(date) = date else {
            continue;
        };
        let span = if day.stops.is_empty() {
            String::new()
        } else {
            let first = day.stops.iter().min_by_key(|s| s.time).unwrap();
            let last = day.stops.iter().max_by_key(|s| s.time).unwrap();
            format!(
                " · {} – {}",
                crate::itinerary::display_time(first),
                crate::itinerary::display_time(last)
            )
        };
        let delta = (*date - today).num_days();
        let relative = match delta {
            0 => "today".to_string(),
            1 => "tomorrow".to_string(),
            d if d > 1 => format!("in {d} days"),
            -1 => "yesterday".to_string(),
            d => format!("{} days ago", -d),
        };
        let label = format!("{} stops{span} · {relative}", day.stops.len());
        push(
            doc.line_end(day.line),
            label,
            format!("{} · {}", date.format("%A, %B %-d, %Y"), date),
        );
        for (i, stop) in day.stops.iter().enumerate() {
            let mut labels = Vec::new();
            if let Some(next) = day.stops.get(i + 1)
                && let Some(seconds) = crate::itinerary::gap(stop, next)
                && seconds > 0
            {
                let arrive_then_depart = stop.kind.is_some_and(|k| k.marker == '<')
                    && next.kind.is_some_and(|k| k.marker == '>');
                labels.push(if arrive_then_depart {
                    format!("{} layover", crate::itinerary::human(seconds))
                } else {
                    format!("{} until {}", crate::itinerary::human(seconds), next.title)
                });
            }
            if let Some((deadline, _)) = crate::itinerary::cancel_by(*date, stop) {
                let passed = deadline.date() < today;
                labels.push(format!(
                    "{}cancel by {}",
                    if passed { "⚠ " } else { "" },
                    deadline.format("%a %b %-d, %I:%M %p")
                ));
            }
            if !labels.is_empty() {
                push(
                    doc.line_end(stop.line),
                    labels.join(" · "),
                    format!(
                        "{} at {} on {}",
                        stop.title,
                        crate::itinerary::display_time(stop),
                        date.format("%A, %B %-d")
                    ),
                );
            }
        }
    }
    for section in &doc.sections {
        let tasks: Vec<_> = doc
            .tasks
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                t.line > section.line
                    && t.line < section.end_line
                    && !doc.tasks.iter().any(|t| t.parent == Some(*i))
            })
            .collect();
        if tasks.is_empty() {
            continue;
        }
        let done = tasks
            .iter()
            .filter(|(i, _)| engine.task_done(path, *i))
            .count();
        let mut effort = 0i64;
        let mut estimates = 0;
        for (i, t) in &tasks {
            if !engine.task_done(path, *i)
                && let Some(attr) = t.attributes.get("estimate")
                && let Ok(Value::Duration(m)) = engine.eval(path, &attr.value)
            {
                effort = effort.saturating_add(m);
                estimates += 1;
            }
        }
        let summary = format!(
            "{done}/{} complete{}",
            tasks.len(),
            if estimates > 0 {
                format!(" · {} estimated left", Value::Duration(effort).display())
            } else {
                String::new()
            }
        );
        let tooltip = format!("`{}` {summary}", crate::charts::bar(done, tasks.len()));
        let label = format!("{} {summary}", crate::charts::gauge(done, tasks.len()));
        push(doc.line_end(section.line), label, tooltip);
    }
    for (i, task) in doc.tasks.iter().enumerate() {
        let mut labels = Vec::new();
        if let Some(attr) = task.attributes.get("timer")
            && let Ok(Value::Timer(timer)) = engine.eval(path, &attr.value)
        {
            labels.push(timer_label(&timer));
        }
        if !engine.task_done(path, i) {
            match engine.blocked(path, i) {
                Ok(blocked) if !blocked.is_empty() => {
                    labels.push(format!("blocked by {}", blocked.join(", ")))
                }
                Err(e) => labels.push(e),
                _ => {}
            }
            for key in ["due", "scheduled", "at"] {
                if let Some(attr) = task.attributes.get(key) {
                    match engine.when(path, &attr.value) {
                        Ok(value) => {
                            let date = value.date().unwrap();
                            let delta = (date - today).num_days();
                            let relative = if delta < 0 && key == "due" {
                                format!("{}d overdue", -delta)
                            } else if delta == 0 {
                                "today".into()
                            } else if delta == 1 {
                                "tomorrow".into()
                            } else {
                                date.to_string()
                            };
                            labels.push(format!("{key} {relative}"));
                        }
                        Err(e) => labels.push(e),
                    }
                }
            }
            if let Some(attr) = task.attributes.get("every") {
                labels.push(format!("repeats every {}", attr.value));
            }
        }
        let children: Vec<_> = doc
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.parent == Some(i))
            .collect();
        let mut tooltip = String::new();
        if !children.is_empty() {
            let done = children
                .iter()
                .filter(|(j, _)| engine.task_done(path, *j))
                .count();
            labels.push(format!(
                "{} {done}/{} subtasks",
                crate::charts::gauge(done, children.len()),
                children.len()
            ));
            tooltip = format!(
                "`{}` {done}/{} subtasks\n\n",
                crate::charts::bar(done, children.len()),
                children.len()
            );
        }
        if !labels.is_empty() {
            tooltip.push_str("Use code actions to complete/reopen tasks or start/pause/reset their timer. Completing a task does not stop its timer.");
            push(doc.line_end(task.line), labels.join(" · "), tooltip);
        }
    }
    for reference in doc.references.iter().filter(|r| r.bracket) {
        if let Ok(value) = engine.eval(path, &reference.expression()) {
            let target = workspace
                .resolve(path, &reference.name)
                .map(|s| s.path)
                .unwrap_or_else(|_| path.into());
            let end = reference.end()
                + doc.line(reference.span.line)[reference.end()..]
                    .find(']')
                    .unwrap_or(0)
                + 1;
            let after = Span::new(reference.span.line, end, end)
                .range(&doc.text)
                .start;
            match value {
                Value::Resource(resource) => push(
                    after,
                    resource.label(&workspace.cache, engine.now.to_utc()),
                    resource.hover(&target, &workspace.cache),
                ),
                Value::Timer(timer) => push(
                    after,
                    timer_label(&timer),
                    "Use Start, Pause, Resume, or Reset timer in code actions.".into(),
                ),
                value if reference.property.is_some() => {
                    push(after, value.display(), reference.expression())
                }
                value => {
                    let tooltip = workspace
                        .resolve(path, &reference.name)
                        .map(|symbol| crate::intelligence::hover(workspace, &symbol, engine.now))
                        .unwrap_or_else(|_| reference.expression());
                    push(after, value.display(), tooltip);
                }
            }
        }
    }
    // Raw and Markdown links to GitHub get the same status badge as named resources.
    for link in &doc.links {
        if crate::resources::github(&link.target).is_none() {
            continue;
        }
        let resource = crate::resources::Resource {
            target: link.target.clone(),
            origin: Some(path.into()),
        };
        push(
            link.span.range(&doc.text).end,
            resource.label(&workspace.cache, engine.now.to_utc()),
            resource.hover(path, &workspace.cache),
        );
    }
    hints.sort_by_key(|h| h.position);
    hints
}
