//! Hovers worded from cached records: a task hovers as its record reads, the
//! same record queries and feature modules are handed.
use crate::{Collection, Records};
use lang::eval::RequestContext;
use lang::eval::engine::Engine;
use lang::stdlib;
use lsp_types::{Hover, HoverContents};
use std::path::Path;

/// A task's hover: its record, as queries and feature modules read it, worded
/// by the stdlib's `task` module.
pub fn task(
    request: &RequestContext<'_>,
    records: &Records,
    path: &Path,
    index: usize,
) -> Option<Hover> {
    let doc = request.workspace().documents().get(path)?;
    let line = doc.tasks.get(index)?.line;
    let text = stdlib::shown(task_text(&mut request.engine(), records, path, index)?);
    Some(Hover {
        contents: HoverContents::Markup(analysis::markup(text)),
        range: Some(doc.line_span(line).range(doc)),
    })
}

/// Task `index`'s hover text as `task.hover` words it from its record.
pub(crate) fn task_text(
    engine: &mut Engine<'_>,
    records: &Records,
    path: &Path,
    index: usize,
) -> Option<stdlib::Presented> {
    let record = records.nth(engine, path, Collection::Tasks, index)?;
    Some(stdlib::task::hover(engine, record))
}
