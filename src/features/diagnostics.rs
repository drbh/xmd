//! Every problem a note can report, and the one vocabulary hosts describe them with.
use crate::{
    document::{Problem, Span},
    engine::{Engine, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use lsp_types::*;
use std::path::Path;

/// The `code` on every diagnostic this crate produces. Queries filter on these
/// names, and the post-processing below reasons about them rather than strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticCode {
    Syntax,
    AmbiguousName,
    UnknownName,
    Resource,
    Evaluation,
    Cycle,
    Dependency,
    Property,
    Attribute,
    /// A feature module's own hook failed; the module id is in the message.
    Module,
}
impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::AmbiguousName => "ambiguous-name",
            Self::UnknownName => "unknown-name",
            Self::Resource => "resource",
            Self::Evaluation => "evaluation",
            Self::Cycle => "cycle",
            Self::Dependency => "dependency",
            Self::Property => "property",
            Self::Attribute => "attribute",
            Self::Module => "module",
        }
    }
    /// Names a token, so a second error for the containing expression is noise.
    fn names_a_token(self) -> bool {
        matches!(self, Self::UnknownName | Self::AmbiguousName)
    }
    fn about_an_expression(self) -> bool {
        matches!(self, Self::Evaluation | Self::Attribute | Self::Dependency)
    }
}
impl std::fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// Recover the enum from a diagnostic the workspace produced.
fn code_of(diagnostic: &Diagnostic) -> Option<DiagnosticCode> {
    let Some(NumberOrString::String(code)) = &diagnostic.code else {
        return None;
    };
    [
        DiagnosticCode::Syntax,
        DiagnosticCode::AmbiguousName,
        DiagnosticCode::UnknownName,
        DiagnosticCode::Resource,
        DiagnosticCode::Evaluation,
        DiagnosticCode::Cycle,
        DiagnosticCode::Dependency,
        DiagnosticCode::Property,
        DiagnosticCode::Attribute,
        DiagnosticCode::Module,
    ]
    .into_iter()
    .find(|c| c.as_str() == code)
}

/// The one severity vocabulary shared by the query catalog and the CLI.
pub fn severity_name(severity: Option<DiagnosticSeverity>) -> &'static str {
    match severity {
        Some(DiagnosticSeverity::WARNING) => "warning",
        Some(DiagnosticSeverity::INFORMATION) => "information",
        Some(DiagnosticSeverity::HINT) => "hint",
        _ => "error",
    }
}

fn diagnostic(
    ws: &Workspace,
    path: &Path,
    span: Span,
    message: String,
    code: DiagnosticCode,
    related: &[Symbol],
) -> Diagnostic {
    Diagnostic {
        range: span.range(&ws.documents[path].text),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("wtf".into()),
        message,
        code: Some(NumberOrString::String(code.as_str().into())),
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
/// The native analysis only: name resolution, evaluation, resources and
/// attributes. Feature modules add their own on top in `collect`.
pub(crate) fn collect_native(
    request: &crate::RequestContext<'_>,
    path: &Path,
    editing: bool,
) -> Vec<Diagnostic> {
    let ws = request.workspace();
    let today = request.today();

    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    let unfinished = |span: Span| {
        editing
            && doc.definitions.iter().any(|d| {
                d.expression
                    && (d.named.span.line == span.line || d.value_span.contains(&doc.text, span))
                    && incomplete(&d.source)
            })
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
            DiagnosticCode::Syntax,
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
                DiagnosticCode::AmbiguousName,
                &candidates,
            ));
            continue;
        }
        let mut engine = request.engine();
        let evaluated = engine.symbol(&symbol);
        if let Ok(Value::Resource(resource)) = &evaluated
            && let Err(message) = resource.url(path)
        {
            let span = match symbol.kind {
                SymbolKind::Definition(i) => doc.definitions[i].value_span,
                _ => named.span,
            };
            issues.push(diagnostic(
                ws,
                path,
                span,
                message,
                DiagnosticCode::Resource,
                &[],
            ));
        }
        if let Err(message) = evaluated {
            if let Some(failure) = engine.failure {
                let incomplete_dependency = editing
                    && ws.documents.get(&failure.path).is_some_and(|dependency| {
                        dependency.definitions.iter().any(|d| {
                            d.expression
                                && d.value_span.contains(&dependency.text, failure.span)
                                && incomplete(&d.source)
                        })
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
                            DiagnosticCode::Evaluation
                        } else {
                            DiagnosticCode::Cycle
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
                        DiagnosticCode::Dependency,
                        &related,
                    ));
                }
            } else {
                issues.push(diagnostic(
                    ws,
                    path,
                    named.span,
                    message,
                    DiagnosticCode::Evaluation,
                    &[],
                ));
            }
        }
    }
    for calculation in &doc.calculations {
        let mut engine = request.engine();
        if let Err(message) = engine.eval_at(path, &calculation.source, calculation.span) {
            let span = engine
                .failure
                .filter(|f| f.path == path)
                .map(|f| f.span)
                .unwrap_or(calculation.span);
            issues.push(diagnostic(
                ws,
                path,
                span,
                message,
                DiagnosticCode::Evaluation,
                &[],
            ));
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
                .filter(|s| s.path == path && ws.named(s).name == reference.name)
                .collect::<Vec<_>>();
            issues.push(diagnostic(
                ws,
                path,
                reference.span,
                message,
                if candidates.is_empty() {
                    DiagnosticCode::UnknownName
                } else {
                    DiagnosticCode::AmbiguousName
                },
                &candidates,
            ));
        } else if reference.property.is_some() {
            let mut engine = request.engine();
            // A failing receiver already carries its own diagnostic.
            if engine.named(path, &reference.name).is_err() {
                continue;
            }
            if let Err(message) = engine.eval_at(path, &reference.expression(), reference.span) {
                let span = Span::new(reference.span.line, reference.span.end + 1, reference.end());
                issues.push(diagnostic(
                    ws,
                    path,
                    span,
                    message,
                    DiagnosticCode::Property,
                    &[],
                ));
            }
        }
    }
    // Keep existing task/date validation, but evaluate attributes at their source spans.
    let mut engine = request.engine();
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
            issues.push(diagnostic(
                ws,
                path,
                span,
                message,
                DiagnosticCode::Dependency,
                related,
            ));
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
                    DiagnosticCode::Attribute,
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
                DiagnosticCode::Attribute,
                &[],
            ));
        }
    }
    // Data that has not been fetched yet is a state, not a mistake in the note.
    for issue in &mut issues {
        if issue.message.starts_with("No cached")
            || issue.message.contains("; run wtf refresh")
            || issue.message.contains("no forecast yet")
        {
            issue.severity = Some(DiagnosticSeverity::WARNING);
        }
    }
    let names_a_token = |d: &Diagnostic| code_of(d).is_some_and(DiagnosticCode::names_a_token);
    issues.sort_by_key(|d| {
        (
            d.range.start,
            d.range.end,
            d.message.clone(),
            !names_a_token(d),
        )
    });
    issues.dedup_by(|a, b| a.range == b.range && a.message == b.message);
    // If name resolution already pinpoints a token, don't add a second error for its containing expression.
    let name_errors = issues
        .iter()
        .filter(|d| names_a_token(d))
        .map(|d| (d.range, d.message.clone()))
        .collect::<Vec<_>>();
    issues.retain(|d| {
        !code_of(d).is_some_and(DiagnosticCode::about_an_expression)
            || !name_errors.iter().any(|(r, m)| {
                r.start.line == d.range.start.line
                    && (d.message == *m || d.message.contains("requires"))
            })
    });
    issues
}
/// Errors only: warnings such as unfetched lookups do not fail checks.
pub(crate) fn problems(request: &crate::RequestContext<'_>, path: &Path) -> Vec<Problem> {
    let ws = request.workspace();
    collect(request, path, false)
        .into_iter()
        .filter(|d| d.severity != Some(DiagnosticSeverity::WARNING))
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

/// Everything wrong with one note, native analysis and feature modules alike.
pub(crate) fn collect(
    request: &crate::RequestContext<'_>,
    path: &Path,
    editing: bool,
) -> Vec<Diagnostic> {
    let mut result = collect_native(request, path, editing);
    result.extend(super::modules::diagnostics(request, path));
    result.sort_by_key(|d| (d.range.start, d.range.end, d.message.clone()));
    result.dedup_by(|a, b| a.range == b.range && a.message == b.message);
    result
}
