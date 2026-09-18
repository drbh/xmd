//! Built-in inlay producers. Each implementation shares the same request context and sink.
use crate::{
    document::Span,
    engine::Value,
    inlays::{InlayContext, InlayFeature, InlaySink},
    workspace::{Symbol, SymbolKind},
};

/// Stable registration order breaks ties between hints at the same position.
pub const BUILTINS: &[&dyn InlayFeature] = &[
    &DefinitionInlays,
    &DecisionInlays,
    &ConstraintInlays,
    &TableCellInlays,
    &ItineraryInlays,
    &ChecklistInlays,
    &TaskInlays,
    &CalculationInlays,
    &ReferenceInlays,
    &LinkInlays,
];

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

pub struct DefinitionInlays;
impl InlayFeature for DefinitionInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let workspace = engine.workspace;
        let path = context.path;
        let doc = context.document;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
        for (i, def) in doc.definitions.iter().enumerate() {
            match engine.symbol(&Symbol {
                path: path.to_path_buf(),
                kind: SymbolKind::Definition(i),
            }) {
                // Resource values are rendered by the LinkInlays adapter.
                Ok(Value::Resource(_)) => {}
                Ok(value) if def.expression => push(
                    def.end.range(&doc.text).start,
                    match &value {
                        Value::Timer(timer) => format!("= {}", timer_label(timer)),
                        Value::Plan(plan) => {
                            std::iter::once(format!("= {}", plan.objective.display()))
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
                                                    .filter_map(|(_, v)| crate::charts::magnitude(
                                                        v
                                                    ))
                                                    .sum()
                                            )
                                            .display()
                                        ),
                                    }
                                }))
                                .collect::<Vec<_>>()
                                .join(" · ")
                        }
                        value => format!("= {}", value.display()),
                    },
                    crate::intelligence::hover_with_links(
                        workspace,
                        &Symbol {
                            path: path.into(),
                            kind: SymbolKind::Definition(i),
                        },
                        engine.now,
                        engine.link_features(),
                    ),
                ),
                _ => {}
            }
        }
    }
}

pub struct DecisionInlays;
impl InlayFeature for DecisionInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let workspace = engine.workspace;
        let path = context.path;
        let doc = context.document;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
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
                        Value::Bool(true) => format!("{} yes", crate::glyphs::CHOSEN),
                        Value::Bool(false) => format!("{} no", crate::glyphs::UNCHOSEN),
                        v => format!("{} {}", crate::glyphs::ARROW, v.display()),
                    };
                    push(
                        cell.span.range(&doc.text).end,
                        shown,
                        format!(
                            "Chosen by plan {}. Use the code action on the plan to write choices into the table.",
                            plan_doc.definitions[plan.definition].named.name
                        ),
                    );
                }
            }
        }
    }
}

pub struct ConstraintInlays;
impl InlayFeature for ConstraintInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let path = context.path;
        let doc = context.document;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
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
                    format!("{} binding", crate::glyphs::ON)
                } else {
                    format!("{} slack {}", crate::glyphs::OFF, result.slack.display())
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
    }
}

pub struct TableCellInlays;
impl InlayFeature for TableCellInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let path = context.path;
        let doc = context.document;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
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
    }
}

pub struct ItineraryInlays;
impl InlayFeature for ItineraryInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let workspace = engine.workspace;
        let doc = context.document;
        let today = engine.today;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
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
            let mut label = format!("{} stops{span} · {relative}", day.stops.len());
            let mut tooltip = format!("{} · {}", date.format("%A, %B %-d, %Y"), date);
            if let Some((places, _)) = &day.places
                && let Some(place) = crate::lookups::day_place(places)
                && let Some(lookup) = workspace
                    .lookups
                    .get(&crate::lookups::forecast_key(&place, *date))
            {
                match crate::lookups::forecast_from(&lookup.value, false) {
                    Ok(forecast) => label.push_str(&format!(" · {}", forecast.display())),
                    Err(e) => label.push_str(&format!(" · {e}")),
                }
                tooltip.push_str(&format!(
                    "\n\nForecast for {place} · {} · {}",
                    crate::resources::ago(lookup.fetched_at, engine.now.to_utc()),
                    lookup.source
                ));
            }
            push(doc.line_end(day.line), label, tooltip);
            for (i, stop) in day.stops.iter().enumerate() {
                let mut labels = Vec::new();
                if let Some(next) = day.stops.get(i + 1)
                    && let Some(seconds) = crate::itinerary::gap(stop, next)
                    && seconds > 0
                {
                    let arrive_then_depart = stop.kind.is_some_and(|k| k.marker == '<')
                        && next.kind.is_some_and(|k| k.marker == '>');
                    labels.push(if arrive_then_depart {
                        format!(
                            "{} {} layover",
                            crate::glyphs::REPEAT,
                            crate::itinerary::human(seconds)
                        )
                    } else {
                        format!(
                            "{} {} until {}",
                            crate::glyphs::ARROW,
                            crate::itinerary::human(seconds),
                            next.title
                        )
                    });
                }
                if let Some((deadline, _)) = crate::itinerary::cancel_by(*date, stop) {
                    let passed = deadline.date() < today;
                    labels.push(format!(
                        "{}cancel by {}",
                        if passed { "! " } else { "" },
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
    }
}

pub struct ChecklistInlays;
impl InlayFeature for ChecklistInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let path = context.path;
        let doc = context.document;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
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
    }
}

pub struct TaskInlays;
impl InlayFeature for TaskInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let path = context.path;
        let doc = context.document;
        let today = engine.today;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
        for (i, task) in doc.tasks.iter().enumerate() {
            let mut labels = Vec::new();
            if let Some(attr) = task.attributes.get("timer")
                && let Ok(Value::Timer(timer)) = engine.eval(path, &attr.value)
            {
                labels.push(timer_label(&timer));
            }
            if !engine.task_done(path, i) {
                match engine.blocked(path, i) {
                    Ok(blocked) if !blocked.is_empty() => labels.push(format!(
                        "{} blocked by {}",
                        crate::glyphs::BLOCKED,
                        blocked.join(", ")
                    )),
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
                                    format!("{} {}d overdue", crate::glyphs::ALERT, -delta)
                                } else if delta == 0 {
                                    "today".into()
                                } else if delta == 1 {
                                    "tomorrow".into()
                                } else {
                                    date.to_string()
                                };
                                labels.push(format!("{} {key} {relative}", crate::glyphs::ARROW));
                            }
                            Err(e) => labels.push(e),
                        }
                    }
                }
                if let Some(attr) = task.attributes.get("every") {
                    labels.push(format!("{} every {}", crate::glyphs::REPEAT, attr.value));
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
    }
}

pub struct CalculationInlays;
impl InlayFeature for CalculationInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let path = context.path;
        let doc = context.document;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
        for calculation in &doc.calculations {
            if let Ok(value) = engine.eval_at(path, &calculation.source, calculation.span) {
                let after = calculation.span.end + usize::from(calculation.bracketed);
                let end = Span::new(calculation.span.line, after, after);
                push(
                    end.range(&doc.text).start,
                    if calculation.bracketed {
                        value.display()
                    } else {
                        format!("= {}", value.display())
                    },
                    format!("`{}` = {}", calculation.source.trim(), value.display()),
                );
            }
        }
    }
}

pub struct ReferenceInlays;
impl InlayFeature for ReferenceInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let workspace = engine.workspace;
        let path = context.path;
        let doc = context.document;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
        for reference in doc.references.iter().filter(|r| r.bracket) {
            if let Ok(value) = engine.eval(path, &reference.expression()) {
                let end = reference.end()
                    + doc.line(reference.span.line)[reference.end()..]
                        .find(']')
                        .unwrap_or(0)
                    + 1;
                let after = Span::new(reference.span.line, end, end)
                    .range(&doc.text)
                    .start;
                match value {
                    Value::Resource(_) => {}
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
                            .map(|symbol| {
                                crate::intelligence::hover_with_links(
                                    workspace,
                                    &symbol,
                                    engine.now,
                                    engine.link_features(),
                                )
                            })
                            .unwrap_or_else(|_| reference.expression());
                        push(after, value.display(), tooltip);
                    }
                }
            }
        }
    }
}

/// Adapts URL semantics to prose links, definitions and references in the shared pipeline.
/// Providers need no knowledge of definitions, references, UTF-16 or editor hosts.
pub struct LinkInlays;
impl InlayFeature for LinkInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let workspace = engine.workspace;
        let path = context.path;
        let doc = context.document;
        let now = engine.now.to_utc();
        let features = engine.link_features();
        let mut time_dependent = false;
        let mut push = |resource: &crate::resources::Resource, position, only_known| {
            let view = resource.presentation(path, &workspace.cache, now, features);
            if only_known && !view.known_link {
                return;
            }
            time_dependent |= view.time_dependent;
            output.push(position, view.label, view.hover);
        };
        for (i, def) in doc.definitions.iter().enumerate() {
            if let Ok(Value::Resource(resource)) = engine.symbol(&Symbol {
                path: path.into(),
                kind: SymbolKind::Definition(i),
            }) {
                push(&resource, def.end.range(&doc.text).start, false);
            }
        }
        for reference in doc.references.iter().filter(|r| r.bracket) {
            if let Ok(Value::Resource(resource)) = engine.eval(path, &reference.expression()) {
                let end = reference.end()
                    + doc.line(reference.span.line)[reference.end()..]
                        .find(']')
                        .unwrap_or(0)
                    + 1;
                let position = Span::new(reference.span.line, end, end)
                    .range(&doc.text)
                    .start;
                push(&resource, position, false);
            }
        }
        // Prose links get badges only when a provider recognizes them.
        for link in &doc.links {
            let resource = crate::resources::Resource {
                target: link.target.clone(),
                origin: Some(path.into()),
            };
            push(&resource, link.span.range(&doc.text).end, true);
        }
        engine.time_dependent |= time_dependent;
    }
}
