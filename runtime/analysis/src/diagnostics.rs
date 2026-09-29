//! Every problem a note can report, and the one vocabulary hosts describe them with.
use lang::common::Span;
use lang::eval::EvalError;
use lang::eval::engine::{Engine, Value};
use lang::eval::timers::Timer;
use lang::eval::{Symbol, SymbolKind, Workspace};
use lsp_types::*;
use std::path::Path;

/// The `code` on every diagnostic this crate produces. Queries filter on these
/// names, and the post-processing below reasons about them rather than strings.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, strum::IntoStaticStr, strum::EnumString,
)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum DiagnosticCode {
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
    pub(crate) fn as_str(self) -> &'static str {
        self.into()
    }
    /// Names a token, so a second error for the containing expression is noise.
    fn names_a_token(self) -> bool {
        matches!(self, Self::UnknownName | Self::AmbiguousName)
    }
    fn about_an_expression(self) -> bool {
        matches!(self, Self::Evaluation | Self::Attribute | Self::Dependency)
    }
}
/// Recover the enum from a diagnostic the workspace produced.
fn code_of(diagnostic: &Diagnostic) -> Option<DiagnosticCode> {
    let Some(NumberOrString::String(code)) = &diagnostic.code else {
        return None;
    };
    code.parse().ok()
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

/// One problem found in a note, before it is placed in the note's text.
struct Issue {
    span: Span,
    code: DiagnosticCode,
    message: String,
    related: Vec<Symbol>,
    /// Reported as a warning: data that has not been fetched yet is a state of
    /// the world, not a mistake in the note.
    pending: bool,
}
impl Issue {
    fn new(span: Span, code: DiagnosticCode, message: String) -> Self {
        Self {
            span,
            code,
            message,
            related: vec![],
            pending: false,
        }
    }
    fn failed(span: Span, code: DiagnosticCode, error: &EvalError) -> Self {
        Self {
            pending: error.is_pending(),
            ..Self::new(span, code, error.to_string())
        }
    }
    fn related(self, related: Vec<Symbol>) -> Self {
        Self { related, ..self }
    }
    fn into_diagnostic(self, ws: &Workspace, path: &Path) -> Diagnostic {
        let related = self.related;
        Diagnostic {
            range: self.span.range(&ws.documents()[path].text),
            severity: Some(if self.pending {
                DiagnosticSeverity::WARNING
            } else {
                DiagnosticSeverity::ERROR
            }),
            source: Some("xmd".into()),
            message: self.message,
            code: Some(NumberOrString::String(self.code.as_str().into())),
            related_information: (!related.is_empty()).then(|| {
                related
                    .iter()
                    .map(|s| DiagnosticRelatedInformation {
                        location: Location {
                            uri: lang::common::uri_from_url(&lang::common::uri(&s.path)),
                            range: ws.named(s).span.range(&ws.documents()[&s.path].text),
                        },
                        message: format!("{} defined here", ws.named(s).name),
                    })
                    .collect()
            }),
            ..Default::default()
        }
    }
}
pub(crate) fn incomplete(source: &str) -> bool {
    if lang::syntax::valid_expression(source) {
        return false;
    }
    let source = source.trim_end();
    source.is_empty()
        || source.ends_with(['+', '-', '*', '/', '(', ',', '.', '!', '=', '&', '|'])
        || source.chars().filter(|c| *c == '(').count()
            > source.chars().filter(|c| *c == ')').count()
        || lang::eval::engine::lex(source).is_err_and(|e| e == "Unclosed string")
}
/// The native analysis only: name resolution, evaluation, resources and
/// attributes. Feature modules add their own on top in `collect`.
pub fn collect_native(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    editing: bool,
) -> Vec<Diagnostic> {
    let ws = request.workspace();
    let today = request.today();

    let Some(doc) = ws.documents().get(path) else {
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
    let symbols = ws.symbols();
    let mut issues = vec![];
    for problem in &doc.problems {
        if editing && problem.message.starts_with("Unclosed") {
            continue;
        }
        issues.push(Issue::new(
            problem.span,
            DiagnosticCode::Syntax,
            problem.message.clone(),
        ));
    }
    for symbol in symbols.iter().filter(|s| s.path == path) {
        let named = ws.named(symbol);
        if unfinished(named.span) {
            continue;
        }
        if let Err(message) = ws.resolve(path, &named.name) {
            let candidates = symbols
                .iter()
                .filter(|s| ws.named(s).name == named.name)
                .cloned()
                .collect();
            issues.push(
                Issue::failed(named.span, DiagnosticCode::AmbiguousName, &message)
                    .related(candidates),
            );
            continue;
        }
        let mut engine = request.engine();
        let evaluated = engine.symbol(symbol);
        if let Ok(Value::Resource(resource)) = &evaluated
            && let Err(message) = resource.url(path)
        {
            let span = match symbol.kind {
                SymbolKind::Definition(i) => doc.definitions[i].value_span,
                _ => named.span,
            };
            issues.push(Issue::failed(
                span,
                DiagnosticCode::Resource,
                &EvalError::Message(message),
            ));
        }
        let Err(message) = evaluated else {
            continue;
        };
        let Some(failure) = engine.failure().cloned() else {
            issues.push(Issue::failed(
                named.span,
                DiagnosticCode::Evaluation,
                &message,
            ));
            continue;
        };
        let incomplete_dependency = editing
            && ws.documents().get(&failure.path).is_some_and(|dependency| {
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
            let code = if failure.message.is_cycle() {
                DiagnosticCode::Cycle
            } else {
                DiagnosticCode::Evaluation
            };
            issues
                .push(Issue::failed(failure.span, code, &failure.message).related(failure.related));
        } else {
            let related = symbols
                .iter()
                .filter(|s| s.path == failure.path && ws.named(s).span.line == failure.span.line)
                .cloned()
                .collect();
            issues.push(
                Issue {
                    message: format!("Dependency error: {message}"),
                    ..Issue::failed(named.span, DiagnosticCode::Dependency, &message)
                }
                .related(related),
            );
        }
    }
    for calculation in &doc.calculations {
        let mut engine = request.engine();
        if let Err(message) = engine.eval_at(path, &calculation.source, calculation.span) {
            let span = engine
                .failure()
                .filter(|f| f.path == path)
                .map(|f| f.span)
                .unwrap_or(calculation.span);
            issues.push(Issue::failed(span, DiagnosticCode::Evaluation, &message));
        }
    }
    for reference in &doc.references {
        if unfinished(reference.span) {
            continue;
        }
        if let Err(message) = lang::eval::tables::resolve_reference(ws, path, reference) {
            let candidates: Vec<_> = symbols
                .iter()
                .filter(|s| s.path == path && ws.named(s).name == reference.name)
                .cloned()
                .collect();
            let code = if candidates.is_empty() {
                DiagnosticCode::UnknownName
            } else {
                DiagnosticCode::AmbiguousName
            };
            issues.push(Issue::failed(reference.span, code, &message).related(candidates));
        } else if reference.property.is_some() {
            let mut engine = request.engine();
            // A failing receiver already carries its own diagnostic.
            if engine.named(path, &reference.name).is_err() {
                continue;
            }
            if let Err(message) = engine.eval_at(path, &reference.expression(), reference.span) {
                let span = Span::new(reference.span.line, reference.span.end + 1, reference.end());
                issues.push(Issue::failed(span, DiagnosticCode::Property, &message));
            }
        }
    }
    // Keep existing task/date validation, but evaluate attributes at their source spans.
    let mut engine = request.engine();
    for (index, task) in doc.tasks.iter().enumerate() {
        engine.clear_failure();
        if let Err(message) = engine.blocked(path, index) {
            let span = task
                .attributes
                .get("after")
                .map(|a| a.value_span)
                .unwrap_or(task.checkbox);
            let related = engine
                .failure()
                .map(|f| f.related.clone())
                .unwrap_or_default();
            issues.push(Issue::failed(span, DiagnosticCode::Dependency, &message).related(related));
        }
        for (key, attr) in &task.attributes {
            if let Some(message) = attribute_error(&mut engine, doc, path, today, index, key, attr)
            {
                issues.push(Issue::failed(
                    attr.value_span,
                    DiagnosticCode::Attribute,
                    &message,
                ));
            }
        }
    }
    for event in &doc.events {
        let attr = &event.attributes["at"];
        if let Err(message) = engine.when(path, &attr.value) {
            issues.push(Issue::failed(
                attr.value_span,
                DiagnosticCode::Attribute,
                &message,
            ));
        }
    }
    let mut issues: Vec<Diagnostic> = issues
        .into_iter()
        .map(|issue| issue.into_diagnostic(ws, path))
        .collect();
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

/// What is wrong with one task attribute, if anything.
fn attribute_error(
    engine: &mut Engine<'_>,
    doc: &lang::model::Document,
    path: &Path,
    today: chrono::NaiveDate,
    task: usize,
    key: &str,
    attr: &lang::model::Attribute,
) -> Option<EvalError> {
    let requires = |satisfied: bool, message: &str| (!satisfied).then(|| message.into());
    match key {
        "due" | "scheduled" | "at" | "repeat_from" => engine.when(path, &attr.value).err(),
        "estimate" => {
            let value = engine.eval_at(path, &attr.value, attr.value_span);
            requires(
                matches!(value, Ok(Value::Duration(s)) if s >= 0),
                "@estimate requires a nonnegative duration, e.g. 20m or 2h",
            )
        }
        "timer" => {
            let value = engine.eval_at(path, &attr.value, attr.value_span);
            requires(
                matches!(&value, Ok(v) if v.downcast::<Timer>().is_some_and(|t| t.origin.is_some()))
                    && lang::model::identifier(&attr.value),
                "@timer requires a named stopwatch or countdown, e.g. @timer(focus)",
            )
        }
        "every" => lang::eval::engine::next_occurrence(&attr.value, today, today)
            .err()
            .or_else(|| {
                doc.tasks
                    .iter()
                    .any(|t| t.parent == Some(task))
                    .then(|| "Put recurrence on individual tasks, not parent checklists".into())
            }),
        _ => None,
    }
}
