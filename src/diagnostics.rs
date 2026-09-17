use crate::{
    document::{Problem, Span},
    engine::{Engine, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate};
use lsp_types::*;
use std::path::Path;

fn diagnostic(
    ws: &Workspace,
    path: &Path,
    span: Span,
    message: String,
    code: &str,
    related: &[Symbol],
) -> Diagnostic {
    Diagnostic {
        range: span.range(&ws.documents[path].text),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("jot".into()),
        message,
        code: Some(NumberOrString::String(code.into())),
        related_information: (!related.is_empty()).then(|| {
            related
                .iter()
                .map(|s| DiagnosticRelatedInformation {
                    location: Location {
                        uri: crate::paths::file_url(&s.path).unwrap(),
                        range: ws.named(s).span.range(&ws.documents[&s.path].text),
                    },
                    message: format!("{} defined here", ws.named(s).name),
                })
                .collect()
        }),
        ..Default::default()
    }
}
pub fn incomplete(source: &str) -> bool {
    if Engine::valid_expression(source) {
        return false;
    }
    let source = source.trim_end();
    source.is_empty()
        || source.ends_with(['+', '-', '*', '/', '(', ',', '.', '!', '=', '&', '|'])
        || source.chars().filter(|c| *c == '(').count()
            > source.chars().filter(|c| *c == ')').count()
        || crate::engine::lex(source).is_err_and(|e| e == "Unclosed string")
}
pub fn collect(
    ws: &Workspace,
    path: &Path,
    today: NaiveDate,
    now: DateTime<FixedOffset>,
    editing: bool,
) -> Vec<Diagnostic> {
    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    let unfinished = |span: Span| {
        editing
            && doc
                .definitions
                .iter()
                .any(|d| d.expression && d.value_span.line == span.line && incomplete(&d.source))
    };
    let mut issues = vec![];
    for problem in &doc.problems {
        if editing && problem.message.starts_with("Unclosed") {
            continue;
        }
        issues.push(diagnostic(
            ws,
            path,
            problem.span,
            problem.message.clone(),
            "syntax",
            &[],
        ));
    }
    for symbol in ws.symbols().into_iter().filter(|s| s.path == path) {
        let named = ws.named(&symbol);
        if unfinished(named.span) {
            continue;
        }
        if let Err(message) = ws.resolve(path, &named.name) {
            let candidates = ws
                .symbols()
                .into_iter()
                .filter(|s| ws.named(s).name == named.name)
                .collect::<Vec<_>>();
            issues.push(diagnostic(
                ws,
                path,
                named.span,
                message,
                "ambiguous-name",
                &candidates,
            ));
            continue;
        }
        let mut engine = Engine::at(ws, now);
        engine.today = today;
        let evaluated = engine.symbol(&symbol);
        if let Ok(Value::Resource(resource)) = &evaluated
            && let Err(message) = resource.url(path)
        {
            let span = match symbol.kind {
                SymbolKind::Definition(i) => doc.definitions[i].value_span,
                _ => named.span,
            };
            issues.push(diagnostic(ws, path, span, message, "resource", &[]));
        }
        if let Err(message) = evaluated {
            if let Some(failure) = engine.failure {
                let incomplete_dependency = editing
                    && ws.documents[&failure.path].definitions.iter().any(|d| {
                        d.expression
                            && d.value_span.line == failure.span.line
                            && incomplete(&d.source)
                    });
                if incomplete_dependency {
                    continue;
                }
                if failure.path == path {
                    issues.push(diagnostic(
                        ws,
                        path,
                        failure.span,
                        failure.message,
                        if failure.related.is_empty() {
                            "evaluation"
                        } else {
                            "cycle"
                        },
                        &failure.related,
                    ));
                } else {
                    let related: Vec<_> = ws
                        .symbols()
                        .into_iter()
                        .filter(|s| {
                            s.path == failure.path && ws.named(s).span.line == failure.span.line
                        })
                        .collect();
                    issues.push(diagnostic(
                        ws,
                        path,
                        named.span,
                        format!("Dependency error: {message}"),
                        "dependency",
                        &related,
                    ));
                }
            } else {
                issues.push(diagnostic(ws, path, named.span, message, "evaluation", &[]));
            }
        }
    }
    for reference in &doc.references {
        if unfinished(reference.span) {
            continue;
        }
        if let Err(message) = crate::tables::resolve_reference(ws, path, reference) {
            let candidates = ws
                .symbols()
                .into_iter()
                .filter(|s| ws.named(s).name == reference.name)
                .collect::<Vec<_>>();
            issues.push(diagnostic(
                ws,
                path,
                reference.span,
                message,
                if candidates.is_empty() {
                    "unknown-name"
                } else {
                    "ambiguous-name"
                },
                &candidates,
            ));
        } else if reference.property.is_some() {
            let mut engine = Engine::at(ws, now);
            // A failing receiver already carries its own diagnostic.
            if engine.named(path, &reference.name).is_err() {
                continue;
            }
            if let Err(message) = engine.eval_at(path, &reference.expression(), reference.span) {
                let span = Span::new(reference.span.line, reference.span.end + 1, reference.end());
                issues.push(diagnostic(ws, path, span, message, "property", &[]));
            }
        }
    }
    // Keep existing task/date validation, but evaluate attributes at their source spans.
    let mut engine = Engine::at(ws, now);
    engine.today = today;
    for (index, task) in doc.tasks.iter().enumerate() {
        engine.failure = None;
        if let Err(message) = engine.blocked(path, index) {
            let span = task
                .attributes
                .get("after")
                .map(|a| a.value_span)
                .unwrap_or(task.checkbox);
            let related = engine
                .failure
                .as_ref()
                .map(|f| f.related.as_slice())
                .unwrap_or(&[]);
            issues.push(diagnostic(ws, path, span, message, "dependency", related));
        }
        for (key, attr) in &task.attributes {
            let error = match key.as_str() {
                "due" | "scheduled" | "at" | "repeat_from" => engine.when(path, &attr.value).err(),
                "estimate" => (!matches!(engine.eval_at(path, &attr.value, attr.value_span), Ok(crate::engine::Value::Duration(s)) if s >= 0)).then(|| "@estimate requires a nonnegative duration, e.g. 20m or 2h".into()),
                "timer" => (!matches!(engine.eval_at(path, &attr.value, attr.value_span), Ok(crate::engine::Value::Timer(t)) if t.origin.is_some() && crate::document::identifier(&attr.value))).then(|| "@timer requires a named stopwatch or countdown, e.g. @timer(focus)".into()),
                "every" => crate::engine::next_occurrence(&attr.value, today, today).err().or_else(|| doc.tasks.iter().any(|t| t.parent == Some(index)).then(|| "Put recurrence on individual tasks, not parent checklists".into())),
                _ => None,
            };
            if let Some(message) = error {
                issues.push(diagnostic(
                    ws,
                    path,
                    attr.value_span,
                    message,
                    "attribute",
                    &[],
                ));
            }
        }
    }
    let dates = crate::itinerary::dates(&doc.days, today);
    let mut previous_date: Option<NaiveDate> = None;
    for (day, date) in doc.days.iter().zip(&dates) {
        let Some(date) = date else {
            issues.push(diagnostic(
                ws,
                path,
                day.date_span,
                format!(
                    "{} {} is not a valid date",
                    crate::itinerary::month_name(day.month),
                    day.day
                ),
                "itinerary",
                &[],
            ));
            continue;
        };
        if let Some((weekday, span)) = &day.weekday
            && *weekday != date.weekday()
        {
            issues.push(diagnostic(
                ws,
                path,
                *span,
                format!(
                    "{} is a {}, not a {}",
                    date.format("%B %-d, %Y"),
                    date.format("%A"),
                    weekday_name(*weekday)
                ),
                "itinerary",
                &[],
            ));
        }
        if let Some(prev) = previous_date
            && *date < prev
        {
            issues.push(diagnostic(
                ws,
                path,
                day.date_span,
                format!("{} comes before the previous day, {prev}", date),
                "itinerary",
                &[],
            ));
        }
        previous_date = Some(*date);
        for pair in day.stops.windows(2) {
            if pair[1].time < pair[0].time {
                issues.push(diagnostic(
                    ws,
                    path,
                    pair[1].time_span,
                    format!(
                        "{} is earlier than the previous stop at {}",
                        crate::itinerary::display_time(&pair[1]),
                        crate::itinerary::display_time(&pair[0])
                    ),
                    "itinerary",
                    &[],
                ));
            }
        }
    }
    for event in &doc.events {
        let attr = &event.attributes["at"];
        if let Err(message) = engine.when(path, &attr.value) {
            issues.push(diagnostic(
                ws,
                path,
                attr.value_span,
                message,
                "attribute",
                &[],
            ));
        }
    }
    issues.sort_by_key(|d| (d.range.start, d.range.end, d.message.clone(), !matches!(&d.code, Some(NumberOrString::String(c)) if c == "unknown-name" || c == "ambiguous-name")));
    issues.dedup_by(|a, b| a.range == b.range && a.message == b.message);
    // If name resolution already pinpoints a token, don't add a second error for its containing expression.
    let name_errors = issues.iter().filter(|d| matches!(&d.code, Some(NumberOrString::String(c)) if c == "unknown-name" || c == "ambiguous-name")).map(|d| (d.range, d.message.clone())).collect::<Vec<_>>();
    issues.retain(|d| !matches!(&d.code, Some(NumberOrString::String(c)) if c == "evaluation" || c == "attribute" || c == "dependency") || !name_errors.iter().any(|(r, m)| r.start.line == d.range.start.line && (d.message == *m || d.message.contains("requires"))));
    issues
}
fn weekday_name(weekday: chrono::Weekday) -> String {
    match weekday {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    }
    .to_string()
}
pub fn problems(ws: &Workspace, path: &Path, today: NaiveDate) -> Vec<Problem> {
    collect(ws, path, today, Local::now().fixed_offset(), false)
        .into_iter()
        .map(|d| {
            let line = ws.documents[path].line(d.range.start.line as usize);
            Problem {
                span: Span::new(
                    d.range.start.line as usize,
                    crate::document::byte_at(line, d.range.start.character).unwrap_or(0),
                    crate::document::byte_at(line, d.range.end.character).unwrap_or(line.len()),
                ),
                message: d.message,
            }
        })
        .collect()
}
