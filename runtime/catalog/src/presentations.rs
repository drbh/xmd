//! Presentation checks: every stdlib `Presents` call the note's own records
//! are shown with, made the way the editor makes it, so a call that fails is
//! a warning on the note. Where the words go the person sees the contract's
//! neutral fallback, never the error; this is where they learn why.
//!
//! The bundled library answers for its own presentations in its tests, so a
//! workspace that brings no modules of its own is not checked: a presentation
//! only starts failing when a module replaces, or is imported by, a bundled
//! one.
use crate::{Records, links};
use lang::common::Span;
use lang::eval::engine::Value;
use lang::eval::plans::PlanValue;
use lang::eval::resources::{Resource, ResourcePresenting};
use lang::eval::{RequestContext, Symbol, SymbolKind};
use lang::stdlib::{self, Presented};
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};
use std::collections::BTreeMap;
use std::path::Path;

/// The first failure of each presentation the note uses, as a warning where
/// it failed.
pub fn presentations(
    request: &RequestContext<'_>,
    records: &Records,
    path: &Path,
) -> Vec<Diagnostic> {
    let ws = request.workspace();
    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    if ws.modules().only_bundled() {
        return vec![];
    }
    let mut failures = Failures::default();
    let mut engine = request.engine();

    for (i, section) in doc.sections.iter().enumerate() {
        let Some(named) = &section.named else {
            continue;
        };
        let symbol = Symbol::new(path, SymbolKind::Section(i));
        if let Ok(Value::Tasks(tasks)) = engine.symbol(&symbol) {
            let done = tasks
                .iter()
                .filter(|(path, index)| engine.task_done(path, *index))
                .count();
            failures.check(
                named.span,
                stdlib::task::checklist(&mut engine, done, tasks.len()),
            );
        }
    }
    for (span, target) in links::targets(doc) {
        let Some(resource) = Resource::parse(target) else {
            continue;
        };
        let record = resource.record(path);
        // A named resource shows a label, which a link module that
        // recognizes it words instead; a prose link shows only that one's.
        let named = !doc.links.iter().any(|link| link.span == span);
        if named
            && engine
                .link_features()
                .presentation(target, ws.cache(), request.now().to_utc())
                .is_none()
        {
            failures.check(span, stdlib::resource::label(&mut engine, record.clone()));
        }
        failures.check(span, stdlib::resource::hover(&mut engine, record.clone()));
        failures.check(span, stdlib::resource::control(&mut engine, record));
    }
    // A record a module built that asks for a lookup offers to refresh it.
    for (line, _) in crate::built::wanted(records, &mut engine, path) {
        failures.check(
            doc.line_span(line),
            stdlib::format::glyph(&mut engine, "refresh"),
        );
    }
    for (i, definition) in doc.definitions.iter().enumerate() {
        let span = definition.named.span;
        let symbol = Symbol::new(path, SymbolKind::Definition(i));
        // A fresh engine, so what it wanted is this definition's alone.
        let mut own = request.engine();
        let value = own.symbol(&symbol);
        let wanted: Vec<_> = own.wanted().cloned().collect();
        if !wanted.is_empty() {
            failures.check(span, stdlib::format::glyph(&mut own, "refresh"));
        }
        for key in wanted {
            if let Some(lookup) = key.lookup(ws.lookups()) {
                let elapsed = (request.now().to_utc() - lookup.fetched_at).num_seconds();
                failures.check(span, stdlib::format::age(&mut own, elapsed));
            }
        }
        if let Some(summary) = analysis::seek_summary(ws, &mut own, &symbol, i) {
            failures.check(span, summary);
        }
        if let Some(contributions) = own.sum_contributions(path, &definition.source) {
            failures.check(span, stdlib::format::series(&mut own, contributions));
        }
        let Ok(value) = value else {
            continue;
        };
        if let Some(plan) = value.downcast::<PlanValue>() {
            let mut snapshot = stdlib::Snapshot {
                modules: ws.modules(),
                now: request.now(),
            };
            failures.check(span, stdlib::plan::hover(&mut snapshot, plan.record(ws)));
            failures.check(span, stdlib::plan::write_title(&mut snapshot));
        }
    }
    for table in &doc.tables {
        for (c, column) in table.columns.iter().enumerate() {
            if table.domains[c].is_some() {
                continue;
            }
            let values = table
                .rows
                .iter()
                .filter_map(|row| row.get(c)?.value.clone().ok().map(Value::from))
                .collect();
            failures.check(column.span, stdlib::format::series(&mut engine, values));
        }
    }
    // Control titles lead with a glyph: refresh (above) on a line that reads
    // a lookup, and the today action on a note's title.
    if doc.line(0).trim_start().starts_with('#') {
        failures.check(doc.line_span(0), stdlib::format::glyph(&mut engine, "flag"));
    }

    failures
        .found
        .into_values()
        .map(|(span, message)| Diagnostic {
            range: span.range(doc),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("module".into())),
            source: Some("xmd".into()),
            message,
            ..Default::default()
        })
        .collect()
}

/// Each presentation's first failure, by contract entry.
#[derive(Default)]
struct Failures {
    found: BTreeMap<(&'static str, &'static str), (Span, String)>,
}
impl Failures {
    fn check<T>(&mut self, span: Span, presented: Presented<T>) {
        let entry = presented.entry;
        if let Err(error) = presented.result {
            self.found
                .entry((entry.module, entry.function))
                .or_insert_with(|| {
                    (
                        span,
                        format!("{}.{}: {error}", entry.module, entry.function),
                    )
                });
        }
    }
}
