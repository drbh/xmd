//! Every problem a note can report, and the one vocabulary hosts describe them with.
use lang::common::Span;
use lang::eval::EvalError;
use lang::eval::engine::Value;
use lang::eval::{Symbol, SymbolKind, Workspace};
use lang::syntax::AttributeValue;
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
    /// A module's own code failed: a feature module's hook, or a stdlib
    /// function native code decides with. The module is in the message.
    Module,
    /// The file's extension does not match what it holds: a file of
    /// definitions is a `.xmd` library, anything else an `.x.md` note.
    FileName,
}
impl DiagnosticCode {
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

/// A module's own failure, as a diagnostic of the `module` code.
pub fn module_problem(severity: DiagnosticSeverity, range: Range, message: String) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(severity),
        source: Some("xmd".into()),
        code: Some(NumberOrString::String(
            <&str>::from(DiagnosticCode::Module).into(),
        )),
        message,
        ..Default::default()
    }
}

/// The one severity vocabulary shared by the `records` crate and the CLI.
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
    /// Reported as information: a convention the note does not follow, which
    /// changes nothing about what it computes.
    advice: bool,
}
impl Issue {
    fn new(span: Span, code: DiagnosticCode, message: String) -> Self {
        Self {
            span,
            code,
            message,
            related: vec![],
            pending: false,
            advice: false,
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
            range: self.span.range(&ws.documents()[path]),
            severity: Some(if self.advice {
                DiagnosticSeverity::INFORMATION
            } else if self.pending {
                DiagnosticSeverity::WARNING
            } else {
                DiagnosticSeverity::ERROR
            }),
            source: Some("xmd".into()),
            message: self.message,
            code: Some(NumberOrString::String(<&str>::from(self.code).into())),
            related_information: (!related.is_empty()).then(|| {
                related
                    .iter()
                    .map(|s| DiagnosticRelatedInformation {
                        location: crate::definition(ws, s),
                        message: format!("{} defined here", ws.named(s).name),
                    })
                    .collect()
            }),
            ..Default::default()
        }
    }
}
/// The naming convention: a file that only defines names, at least one of
/// them a function, is a library for other files to import and is named
/// `.xmd`; anything with note content (prose, headings, tasks, tables) is a
/// working note, named `.x.md`. Both are read the same way, so this is
/// advice about the name, never an error.
fn file_name(doc: &lang::document::Document, path: &Path) -> Option<Issue> {
    let stem = lang::common::note_stem(path)?;
    let lines: Vec<&str> = doc.text().lines().collect();
    // Rows that belong to a definition written as its own line, `name := …`;
    // a table, or the table a form takes, laid out under a name is note
    // content.
    let mut defined = vec![false; lines.len()];
    for (index, d) in doc.definitions().iter().enumerate() {
        let line = lines.get(d.named.span.line).copied().unwrap_or("");
        let own_line = d.expression && d.named.span.start == line.len() - line.trim_start().len();
        if own_line && doc.grid_of(index).is_none() {
            let (first, last) = doc.definition_rows(index);
            defined[first..=last.min(lines.len().saturating_sub(1))].fill(true);
        }
    }
    let mut comment = false;
    let content = lines.iter().enumerate().find(|&(row, line)| {
        let trimmed = line.trim();
        let in_comment = comment || trimmed.starts_with("<!--");
        comment = in_comment && !trimmed.contains("-->");
        !(trimmed.is_empty() || in_comment || trimmed.starts_with("//") || defined[row])
    });
    if lang::common::is_library(path) {
        let (row, line) = content?;
        let start = line.len() - line.trim_start().len();
        return Some(Issue {
            advice: true,
            ..Issue::new(
                Span::new(row, start, line.trim_end().len()),
                DiagnosticCode::FileName,
                format!(
                    "This line is note content, but .{} files only define names for other files to import; a working note is named {}",
                    lang::common::LIBRARY_EXTENSION,
                    lang::common::note_file(stem),
                ),
            )
        });
    }
    if content.is_some() {
        return None;
    }
    let first = doc
        .definitions()
        .iter()
        .find(|d| d.expression && d.source.starts_with("fn"))?;
    Some(Issue {
        advice: true,
        ..Issue::new(
            first.named.span,
            DiagnosticCode::FileName,
            format!(
                "This file only defines names, including functions, so it is a library: name it {} and keep .{} for working notes",
                lang::common::library_file(stem),
                lang::common::EXTENSION,
            ),
        )
    })
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
        || lang::syntax::lex(source).is_err_and(|e| e == "Unclosed string")
}
/// The native analysis only: name resolution, evaluation, resources and
/// attributes. Feature modules add their own on top in `collect`.
pub fn collect_native(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    editing: bool,
) -> Vec<Diagnostic> {
    let ws = request.workspace();
    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    // The definitions still being typed, found once rather than per name.
    let typing: Vec<_> = doc
        .definitions()
        .iter()
        .filter(|d| editing && d.expression && incomplete(&d.source))
        .collect();
    let unfinished = |span: Span| {
        typing
            .iter()
            .any(|d| d.named.span.line == span.line || d.value_span.contains(doc, span))
    };
    let symbols = ws.symbols();
    let mut issues: Vec<Issue> = file_name(doc, path).into_iter().collect();
    for problem in doc.problems() {
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
            && let Err(message) = resource.url(path, ws.home())
        {
            let span = match symbol.kind {
                SymbolKind::Definition(i) => doc.definitions()[i].value_span,
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
                dependency.definitions().iter().any(|d| {
                    d.expression
                        && d.value_span.contains(dependency, failure.span)
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
    for calculation in doc.calculations() {
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
    for reference in doc.references() {
        if unfinished(reference.span) {
            continue;
        }
        if let Err(message) = lang::eval::tables::resolve_reference(ws, path, reference) {
            // A prelude function is a name without being a symbol.
            if ws.prelude_name(path, &reference.name) {
                continue;
            }
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
    // The attributes modules declare, wherever they are live: each
    // evaluated as what it holds, at its source span, a failure being its
    // value's problem. Dependencies that fail (a cycle, a condition that is
    // no Boolean) are a dependency problem, with the items they walk.
    let mut engine = request.engine();
    for line in doc.claimed() {
        let item = line
            .checkbox
            .then(|| {
                doc.tasks()
                    .binary_search_by_key(&line.line, |t| t.line)
                    .ok()
            })
            .flatten();
        for (declared, attr) in doc.live_attributes(line) {
            engine.clear_failure();
            let Err(message) = engine.attribute(path, declared, item, attr) else {
                continue;
            };
            let key = &declared.key;
            let issue = match declared.value {
                AttributeValue::Dependencies => {
                    let related = engine
                        .failure()
                        .map(|f| f.related.clone())
                        .unwrap_or_default();
                    Issue::failed(attr.value_span, DiagnosticCode::Dependency, &message)
                        .related(related)
                }
                AttributeValue::Duration => Issue::failed(
                    attr.value_span,
                    DiagnosticCode::Attribute,
                    &EvalError::Message(format!(
                        "@{key} requires a nonnegative duration, e.g. 20m or 2h"
                    )),
                ),
                _ => Issue::failed(attr.value_span, DiagnosticCode::Attribute, &message),
            };
            issues.push(issue);
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
